//! Ports and input/output (R7RS §6.13), forked from s1's `ports.rs`.
//!
//! A port is a heap object holding a `PortKind` (`io.rs`) in a `RefCell`;
//! every procedure here works on the port in place, so all references to
//! a port share its position, output and open/closed state.
//!
//! The current input, output and error ports live in the VM (`Vm::ports`),
//! so the procedures that default to them are VM primitives. The procedures
//! that return them also speak the protocol `parameterize` uses for
//! parameter objects (`make-parameter` in `scheme/core.scm`, as in s1), so
//! they can be parameterized.

use std::cell::RefCell;
use std::io::Write;

use super::{Error, ErrorKind, err, Primitive, bytevector_arg, char_arg, expected, fixnum, index_arg, kind, prim, range_args, string_arg, vm_prim};
use crate::globals::{TopBinding, cell_value, env_of};
use crate::heap::{Heap, Obj};
use crate::io::{PortKind, new_string_port_input};
use crate::printer::{display_value, print_value, write_shared_value, write_simple_value};
use crate::reader::{ParseError, parse};
use crate::value::Value;
use crate::vm::Vm;

/// Indexes of the current ports in `Vm::ports`.
pub const INPUT: usize = 0;
pub const OUTPUT: usize = 1;
pub const ERROR: usize = 2;

/// New ports for standard input, output and error: the initial current
/// ports.
pub fn standard_ports(heap: &mut Heap) -> [Value; 3] {
    [heap.port(PortKind::Stdin), heap.port(PortKind::Stdout), heap.port(PortKind::Stderr)]
}

pub static PRIMITIVES: &[Primitive] = &[
    vm_prim("current-input-port", 0, Some(2), |vm, h, a| current_port(vm, h, a, INPUT, "%check-input-port")),
    vm_prim("current-output-port", 0, Some(2), |vm, h, a| current_port(vm, h, a, OUTPUT, "%check-output-port")),
    vm_prim("current-error-port", 0, Some(2), |vm, h, a| current_port(vm, h, a, ERROR, "%check-output-port")),
    // The current ports' converters, which check what parameterize binds.
    prim("%check-input-port", 1, Some(1), |_, a| match kind(a[0]) {
        Some(Obj::Port(p)) if p.borrow().is_input() => Ok(a[0]),
        _ => Err(expected("current-input-port", "an input port", a[0])),
    }),
    prim("%check-output-port", 1, Some(1), |_, a| match kind(a[0]) {
        Some(Obj::Port(p)) if p.borrow().is_output() => Ok(a[0]),
        _ => Err(expected("current output port", "an output port", a[0])),
    }),
    prim("port?", 1, Some(1), |_, a| port_test(a[0], |_| true)),
    prim("input-port?", 1, Some(1), |_, a| port_test(a[0], PortKind::is_input)),
    prim("output-port?", 1, Some(1), |_, a| port_test(a[0], PortKind::is_output)),
    prim("textual-port?", 1, Some(1), |_, a| port_test(a[0], PortKind::is_textual)),
    prim("binary-port?", 1, Some(1), |_, a| port_test(a[0], |p| !p.is_textual())),
    prim("input-port-open?", 1, Some(1), |_, a| port_test(a[0], |p| p.is_input() && p.is_open())),
    prim("output-port-open?", 1, Some(1), |_, a| port_test(a[0], |p| p.is_output() && p.is_open())),
    prim("eof-object", 0, Some(0), |_, _| Ok(Value::EOF)),
    prim("eof-object?", 1, Some(1), |_, a| Ok(Value::bool(a[0].is_eof()))),
    prim("open-input-string", 1, Some(1), |h, a| {
        let s = string_arg(a[0], "open-input-string")?.borrow().to_string();
        Ok(h.port(new_string_port_input(&s)))
    }),
    prim("open-output-string", 0, Some(0), |h, _| Ok(h.port(PortKind::StringPortOutput { content: String::new() }))),
    prim("get-output-string", 1, Some(1), |h, a| {
        let content = match &*port(a[0], "get-output-string")?.borrow() {
            PortKind::StringPortOutput { content } => content.clone(),
            _ => return Err("get-output-string: not a port made by open-output-string".into()),
        };
        Ok(h.string(&content))
    }),
    prim("open-input-bytevector", 1, Some(1), |h, a| {
        let bytes = bytevector_arg(a[0], "open-input-bytevector")?.iter().map(|b| b.get()).collect();
        Ok(h.port(PortKind::BytevectorInput { bytes, pos: 0 }))
    }),
    prim("open-output-bytevector", 0, Some(0), |h, _| Ok(h.port(PortKind::BytevectorOutput { bytes: Vec::new() }))),
    prim("get-output-bytevector", 1, Some(1), |h, a| {
        let bytes = match &*port(a[0], "get-output-bytevector")?.borrow() {
            PortKind::BytevectorOutput { bytes } => bytes.clone(),
            _ => return Err("get-output-bytevector: not a port made by open-output-bytevector".into()),
        };
        Ok(h.bytevector(bytes))
    }),
    prim("open-input-file", 1, Some(1), |h, a| open_input(h, a[0], false)),
    prim("open-binary-input-file", 1, Some(1), |h, a| open_input(h, a[0], true)),
    prim("open-output-file", 1, Some(1), |h, a| open_output(h, a[0], false)),
    prim("open-binary-output-file", 1, Some(1), |h, a| open_output(h, a[0], true)),
    prim("close-port", 1, Some(1), |_, a| close(a[0], "close-port", |_| true)),
    prim("close-input-port", 1, Some(1), |_, a| close(a[0], "close-input-port", PortKind::is_input)),
    prim("close-output-port", 1, Some(1), |_, a| close(a[0], "close-output-port", PortKind::is_output)),
    prim("file-exists?", 1, Some(1), |_, a| {
        Ok(Value::bool(std::path::Path::new(string_arg(a[0], "file-exists?")?.borrow().as_str()).exists()))
    }),
    prim("delete-file", 1, Some(1), |_, a| {
        let name = string_arg(a[0], "delete-file")?.borrow().to_string();
        std::fs::remove_file(&name).map_err(|e| file_error(format!("delete-file: could not delete file: {}", e), a[0]))?;
        Ok(Value::UNSPECIFIED)
    }),
    vm_prim("read", 0, Some(1), |vm, h, a| {
        let p = port(current(vm, a, 0, INPUT), "read")?;
        let mut k = p.borrow_mut();
        let result = match &*k {
            PortKind::Stdin | PortKind::StringPortInput { .. } => parse(h, &mut k),
            PortKind::Closed { .. } => return Err(closed("read")),
            _ => return Err("read: not a textual input port".into()),
        };
        match result {
            Ok(datum) => Ok(datum),
            Err(ParseError::Eof) => Ok(Value::EOF),
            Err(ParseError::Syntax(e)) => {
                Err(Error { kind: ErrorKind::Read, message: format!("read: syntax error: {}", e), irritants: Vec::new() })
            }
        }
    }),
    vm_prim("read-char", 0, Some(1), |vm, _, a| Ok(char_or_eof(get_char(current(vm, a, 0, INPUT), "read-char", false)?))),
    vm_prim("peek-char", 0, Some(1), |vm, _, a| Ok(char_or_eof(get_char(current(vm, a, 0, INPUT), "peek-char", true)?))),
    vm_prim("read-line", 0, Some(1), read_line),
    vm_prim("read-string", 1, Some(2), |vm, h, a| {
        let k = index_arg(a[0], "read-string")?;
        let p = current(vm, a, 1, INPUT);
        let mut out = String::new();
        while out.chars().count() < k {
            match get_char(p, "read-string", false)? {
                Some(c) => out.push(c),
                None => break,
            }
        }
        Ok(if out.is_empty() && k > 0 { Value::EOF } else { h.string(&out) })
    }),
    vm_prim("char-ready?", 0, Some(1), |vm, _, a| {
        match &*port(current(vm, a, 0, INPUT), "char-ready?")?.borrow() {
            PortKind::StringPortInput { .. } => Ok(Value::TRUE),
            PortKind::Stdin => Ok(Value::FALSE),
            PortKind::Closed { .. } => Err(closed("char-ready?")),
            _ => Err("char-ready?: not a textual input port".into()),
        }
    }),
    vm_prim("write", 1, Some(2), |vm, _, a| put_str(current(vm, a, 1, OUTPUT), &print_value(a[0]), "write")),
    vm_prim("write-shared", 1, Some(2), |vm, _, a| put_str(current(vm, a, 1, OUTPUT), &write_shared_value(a[0]), "write-shared")),
    vm_prim("write-simple", 1, Some(2), |vm, _, a| put_str(current(vm, a, 1, OUTPUT), &write_simple_value(a[0]), "write-simple")),
    vm_prim("display", 1, Some(2), |vm, _, a| put_str(current(vm, a, 1, OUTPUT), &display_value(a[0]), "display")),
    vm_prim("newline", 0, Some(1), |vm, _, a| put_str(current(vm, a, 0, OUTPUT), "\n", "newline")),
    vm_prim("write-char", 1, Some(2), |vm, _, a| {
        let c = char_arg(a[0], "write-char")?;
        put_str(current(vm, a, 1, OUTPUT), &c.to_string(), "write-char")
    }),
    vm_prim("write-string", 1, Some(4), |vm, _, a| {
        let chars: Vec<char> = string_arg(a[0], "write-string")?.borrow().chars().collect();
        let (start, end) = range_args(a, 2, chars.len(), "write-string")?;
        put_str(current(vm, a, 1, OUTPUT), &chars[start..end].iter().collect::<String>(), "write-string")
    }),
    vm_prim("write-u8", 1, Some(2), |vm, _, a| {
        let b = a[0].as_fixnum().and_then(|n| u8::try_from(n).ok()).ok_or_else(|| expected("write-u8", "a byte (0 to 255)", a[0]))?;
        put_bytes(current(vm, a, 1, OUTPUT), &[b], "write-u8")
    }),
    vm_prim("write-bytevector", 1, Some(4), |vm, _, a| {
        let bv = bytevector_arg(a[0], "write-bytevector")?;
        let (start, end) = range_args(a, 2, bv.len(), "write-bytevector")?;
        let bytes: Vec<u8> = bv[start..end].iter().map(|b| b.get()).collect();
        put_bytes(current(vm, a, 1, OUTPUT), &bytes, "write-bytevector")
    }),
    vm_prim("flush-output-port", 0, Some(1), |vm, _, a| {
        match &mut *port(current(vm, a, 0, OUTPUT), "flush-output-port")?.borrow_mut() {
            PortKind::Stdout => std::io::stdout().flush().ok(),
            PortKind::Stderr => std::io::stderr().flush().ok(),
            PortKind::FileOutput { file, .. } => file.flush().ok(),
            PortKind::Closed { .. } => return Err(closed("flush-output-port")),
            _ => None,
        };
        Ok(Value::UNSPECIFIED)
    }),
    vm_prim("read-u8", 0, Some(1), |vm, _, a| binary_read(current(vm, a, 0, INPUT), "read-u8", true)),
    vm_prim("peek-u8", 0, Some(1), |vm, _, a| binary_read(current(vm, a, 0, INPUT), "peek-u8", false)),
    vm_prim("u8-ready?", 0, Some(1), |vm, _, a| {
        with_binary_input(current(vm, a, 0, INPUT), "u8-ready?", |_, _| ())?;
        Ok(Value::TRUE)
    }),
    vm_prim("read-bytevector", 1, Some(2), |vm, h, a| {
        let k = index_arg(a[0], "read-bytevector")?;
        let bytes = with_binary_input(current(vm, a, 1, INPUT), "read-bytevector", |bytes, pos| {
            let n = k.min(bytes.len() - *pos);
            let out = bytes[*pos..*pos + n].to_vec();
            *pos += n;
            out
        })?;
        Ok(if bytes.is_empty() && k > 0 { Value::EOF } else { h.bytevector(bytes) })
    }),
    vm_prim("read-bytevector!", 1, Some(4), |vm, _, a| {
        let target = bytevector_arg(a[0], "read-bytevector!")?;
        let (start, end) = range_args(a, 2, target.len(), "read-bytevector!")?;
        let src = with_binary_input(current(vm, a, 1, INPUT), "read-bytevector!", |bytes, pos| {
            let n = (end - start).min(bytes.len() - *pos);
            let out = bytes[*pos..*pos + n].to_vec();
            *pos += n;
            out
        })?;
        if src.is_empty() && end > start {
            return Ok(Value::EOF);
        }
        target[start..start + src.len()].iter().zip(&src).for_each(|(c, &b)| c.set(b));
        Ok(fixnum(src.len()))
    }),
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn port<'a>(v: Value, who: &str) -> Result<&'a RefCell<PortKind>, Error> {
    match kind(v) {
        Some(Obj::Port(p)) => Ok(p),
        _ => Err(expected(who, "a port", v)),
    }
}

/// `(current-output-port)` and friends. `(p %param-converter)` returns the
/// converter (the global `checker`), and `(p %param-set port)` sets it, for
/// `parameterize`.
fn current_port(vm: &mut Vm, heap: &mut Heap, args: &[Value], which: usize, checker: &str) -> Result<Value, Error> {
    match args {
        [] => Ok(vm.ports[which]),
        [m] if m.eq(heap.intern("%param-converter")) => {
            let name = heap.intern(checker).as_ref().unwrap();
            match env_of(heap.system.unwrap()).borrow().lookup(name) {
                Some(TopBinding::Variable(cell)) => Ok(cell_value(cell).get()),
                _ => Err(err!("{} is not defined", checker)),
            }
        }
        [m, port] if m.eq(heap.intern("%param-set")) => {
            vm.ports[which] = *port;
            Ok(Value::UNSPECIFIED)
        }
        _ => Err("current port: expects no arguments".into()),
    }
}

/// The port argument at `args[i]`, or the current port `which`.
fn current(vm: &Vm, args: &[Value], i: usize, which: usize) -> Value {
    args.get(i).copied().unwrap_or(vm.ports[which])
}

/// A file error (R7RS `file-error?`), naming the file as its irritant.
fn file_error(message: String, file: Value) -> Error {
    Error { kind: ErrorKind::File, message, irritants: vec![file] }
}

fn closed(who: &str) -> Error {
    err!("{}: the port is closed", who)
}

fn port_test(v: Value, test: fn(&PortKind) -> bool) -> Result<Value, Error> {
    Ok(Value::bool(matches!(kind(v), Some(Obj::Port(p)) if test(&p.borrow()))))
}

/// Write text to a textual output port.
fn put_str(port_value: Value, text: &str, who: &str) -> Result<Value, Error> {
    match &mut *port(port_value, who)?.borrow_mut() {
        PortKind::Stdout => print!("{}", text),
        PortKind::Stderr => {
            // Keep stderr output in order with buffered stdout output.
            std::io::stdout().flush().ok();
            eprint!("{}", text);
        }
        PortKind::StringPortOutput { content } => content.push_str(text),
        PortKind::FileOutput { name, file, binary: false } => {
            file.write_all(text.as_bytes()).map_err(|e| err!("{}: could not write to {}: {}", who, name, e))?
        }
        PortKind::Closed { .. } => return Err(closed(who)),
        _ => return Err(err!("{}: not a textual output port", who)),
    }
    Ok(Value::UNSPECIFIED)
}

fn put_bytes(port_value: Value, bytes: &[u8], who: &str) -> Result<Value, Error> {
    match &mut *port(port_value, who)?.borrow_mut() {
        PortKind::BytevectorOutput { bytes: out } => out.extend_from_slice(bytes),
        PortKind::FileOutput { name, file, binary: true } => {
            file.write_all(bytes).map_err(|e| err!("{}: could not write to {}: {}", who, name, e))?
        }
        PortKind::Closed { .. } => return Err(closed(who)),
        _ => return Err(err!("{}: not a binary output port", who)),
    }
    Ok(Value::UNSPECIFIED)
}

/// The next character of a textual input port, consumed or not.
fn get_char(port_value: Value, who: &str, peek: bool) -> Result<Option<char>, Error> {
    let mut k = port(port_value, who)?.borrow_mut();
    match &*k {
        PortKind::Stdin | PortKind::StringPortInput { .. } => {
            let c = k.next_char_utf8();
            if peek && let Some(c) = c {
                k.unread_char(c);
            }
            Ok(c)
        }
        PortKind::Closed { .. } => Err(closed(who)),
        _ => Err(err!("{}: not a textual input port", who)),
    }
}

fn char_or_eof(c: Option<char>) -> Value {
    c.map_or(Value::EOF, Value::char)
}

/// A line, without its terminator (`\n`, `\r` or `\r\n`); the eof object at
/// the end of input.
fn read_line(vm: &mut Vm, heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let p = current(vm, args, 0, INPUT);
    let mut line = String::new();
    let mut any = false;
    loop {
        match get_char(p, "read-line", false)? {
            None => break,
            Some('\n') => {
                any = true;
                break;
            }
            Some('\r') => {
                any = true;
                if get_char(p, "read-line", true)? == Some('\n') {
                    get_char(p, "read-line", false)?;
                }
                break;
            }
            Some(c) => {
                any = true;
                line.push(c);
            }
        }
    }
    Ok(if any { heap.string(&line) } else { Value::EOF })
}

/// Run `f` on a binary input port's bytes and position.
fn with_binary_input<T>(port_value: Value, who: &str, f: impl FnOnce(&[u8], &mut usize) -> T) -> Result<T, Error> {
    match &mut *port(port_value, who)?.borrow_mut() {
        PortKind::BytevectorInput { bytes, pos } => Ok(f(bytes, pos)),
        PortKind::Closed { .. } => Err(closed(who)),
        _ => Err(err!("{}: not a binary input port", who)),
    }
}

fn binary_read(port_value: Value, who: &str, consume: bool) -> Result<Value, Error> {
    let b = with_binary_input(port_value, who, |bytes, pos| {
        let b = bytes.get(*pos).copied();
        if consume && b.is_some() {
            *pos += 1;
        }
        b
    })?;
    Ok(b.map_or(Value::EOF, |b| fixnum(b as usize)))
}

fn open_input(heap: &mut Heap, name: Value, binary: bool) -> Result<Value, Error> {
    let who = if binary { "open-binary-input-file" } else { "open-input-file" };
    let file_name = name;
    let name = string_arg(name, who)?.borrow().to_string();
    let err = |e: std::io::Error| file_error(format!("{}: could not open file: {}", who, e), file_name);
    let kind = if binary {
        PortKind::BytevectorInput { bytes: std::fs::read(&name).map_err(err)?, pos: 0 }
    } else {
        new_string_port_input(&std::fs::read_to_string(&name).map_err(err)?)
    };
    Ok(heap.port(kind))
}

fn open_output(heap: &mut Heap, name: Value, binary: bool) -> Result<Value, Error> {
    let who = if binary { "open-binary-output-file" } else { "open-output-file" };
    let file_name = name;
    let name = string_arg(name, who)?.borrow().to_string();
    let file = std::fs::File::create(&name).map_err(|e| file_error(format!("{}: could not open file: {}", who, e), file_name))?;
    Ok(heap.port(PortKind::FileOutput { name, file: std::io::BufWriter::new(file), binary }))
}

/// Close a port (closing an output file), after checking it is the right
/// kind.
fn close(port_value: Value, who: &str, right_kind: fn(&PortKind) -> bool) -> Result<Value, Error> {
    let mut k = port(port_value, who)?.borrow_mut();
    if !right_kind(&k) {
        return Err(expected(who, if who == "close-input-port" { "an input port" } else { "an output port" }, port_value));
    }
    *k = k.closed();
    Ok(Value::UNSPECIFIED)
}
