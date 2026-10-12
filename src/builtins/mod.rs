//! Procedures written in Rust (D9, plan/03 §11), one module per R7RS
//! chapter, forked from s1's `builtin/*`.
//!
//! A builtin takes its arguments as a slice and returns a value or an error
//! message. It never calls Scheme code, so it never re-enters the VM. The VM
//! checks the argument count against `min`/`max` before calling, so the
//! functions can index `args` directly.
//!
//! A few procedures need more than the heap: `dis` (and later `eval`) must
//! compile, which needs the top-level environment, and the port procedures
//! need the current ports, which live in the VM. These are *VM primitives*
//! (03 §11): still first-class procedure values, but called with the VM's
//! context. `apply`, `%call/cc` and `%call/ec` are carried out by the VM's
//! call instructions themselves (`PrimFn::Apply` and the rest).

mod char;
mod control;
pub mod list;
pub mod number;
mod port;
pub mod predicate;
mod record;
mod string;
mod system;
pub mod vector;

use std::cell::{Cell, RefCell};

use crate::globals::Env;
use crate::heap::{Heap, Obj, list_to_vec};
use crate::printer::print_value;
use crate::sstring::SString;
use crate::value::Value;
use crate::vm::Vm;

pub use port::standard_ports;
pub use system::set_command_line;

/// What a builtin returns when it fails. The VM raises it as an error
/// object (`Obj::Error`), which `guard` can catch. Most errors are just a
/// message; file and read errors say so (R7RS's `file-error?`,
/// `read-error?`), and may have irritants.
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub irritants: Vec<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorKind {
    General,
    File,
    Read,
    /// Not an error object's kind: `%abort` returns it, and the VM passes
    /// it straight out of `run` instead of raising it.
    Uncaught,
}

impl From<String> for Error {
    fn from(message: String) -> Error {
        Error { kind: ErrorKind::General, message, irritants: Vec::new() }
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Error {
        Error::from(message.to_string())
    }
}

/// An `Error` with a formatted message, like `format!`.
macro_rules! err {
    ($($arg:tt)*) => { $crate::builtins::Error::from(format!($($arg)*)) };
}
pub(crate) use err;

pub type BuiltinFn = fn(&mut Heap, &[Value]) -> Result<Value, Error>;

/// A VM primitive's code: it also gets the VM and the environment the
/// running code belongs to.
// ponytail: returns a value, which is all these need. P7 changes this to
// return an `Action` (03 §11), so `eval` can tail-call the code it compiles
// and `%call/cc` can resume a continuation.
pub type VmFn = fn(&mut Vm, &mut Heap, &[Value]) -> Result<Value, Error>;

pub enum PrimFn {
    Builtin(BuiltinFn),
    Vm(VmFn),
    /// `apply`, `%call/cc` and `%call/ec`, which the VM's call instructions
    /// carry out (`vm/machine.rs`).
    Apply,
    CallCc,
    CallEc,
    /// `eval`, which the call instruction carries out too: it compiles the
    /// expression and calls the result in place of itself.
    Eval,
}

/// A procedure written in Rust: its name, its argument count, and its code.
pub struct Primitive {
    pub name: &'static str,
    pub min: usize,
    /// None for "any number".
    pub max: Option<usize>,
    pub f: PrimFn,
}

pub(crate) const fn prim(name: &'static str, min: usize, max: Option<usize>, f: BuiltinFn) -> Primitive {
    Primitive { name, min, max, f: PrimFn::Builtin(f) }
}

pub(crate) const fn vm_prim(name: &'static str, min: usize, max: Option<usize>, f: VmFn) -> Primitive {
    Primitive { name, min, max, f: PrimFn::Vm(f) }
}

/// Define every builtin in `env`, the system environment.
pub fn install(heap: &mut Heap, env: &mut Env) {
    for table in [
        control::PRIMITIVES,
        number::PRIMITIVES,
        predicate::PRIMITIVES,
        list::PRIMITIVES,
        char::PRIMITIVES,
        string::PRIMITIVES,
        vector::PRIMITIVES,
        record::PRIMITIVES,
        port::PRIMITIVES,
        system::PRIMITIVES,
        crate::libraries::PRIMITIVES,
        crate::compile::data::PRIMITIVES,
    ] {
        for p in table {
            let v = heap.primitive(p);
            env.define(heap, p.name, v);
        }
    }
}

// ---------------------------------------------------------------------------
// Argument helpers, shared by the modules
// ---------------------------------------------------------------------------

pub fn kind<'a>(v: Value) -> Option<&'a Obj> {
    v.as_obj().map(|o| &o.kind)
}

/// `who: expected WHAT, got V`, the type error s1 reports.
pub fn expected(who: &str, what: &str, v: Value) -> Error {
    err!("{}: expected {}, got {}", who, what, print_value(v))
}

pub fn pair_arg<'a>(v: Value, who: &str) -> Result<(&'a Cell<Value>, &'a Cell<Value>), Error> {
    match kind(v) {
        Some(Obj::Pair { car, cdr }) => Ok((car, cdr)),
        _ => Err(expected(who, "a pair", v)),
    }
}

pub fn char_arg(v: Value, who: &str) -> Result<char, Error> {
    v.as_char().ok_or_else(|| expected(who, "a character", v))
}

pub fn string_arg<'a>(v: Value, who: &str) -> Result<&'a RefCell<SString>, Error> {
    match kind(v) {
        Some(Obj::Str(s)) => Ok(s),
        _ => Err(expected(who, "a string", v)),
    }
}

pub fn vector_arg<'a>(v: Value, who: &str) -> Result<&'a [Cell<Value>], Error> {
    match kind(v) {
        Some(Obj::Vector(items)) => Ok(items),
        _ => Err(expected(who, "a vector", v)),
    }
}

pub fn bytevector_arg<'a>(v: Value, who: &str) -> Result<&'a [Cell<u8>], Error> {
    match kind(v) {
        Some(Obj::Bytevector(b)) => Ok(b),
        _ => Err(expected(who, "a bytevector", v)),
    }
}

/// A non-negative fixnum: an index, a length or a count.
pub fn index_arg(v: Value, who: &str) -> Result<usize, Error> {
    v.as_fixnum()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| err!("{}: index must be a non-negative integer, got {}", who, print_value(v)))
}

/// The elements of the proper list `v`.
pub fn list_arg(v: Value, who: &str) -> Result<Vec<Value>, Error> {
    list_to_vec(v).ok_or_else(|| expected(who, "a proper list", v))
}

/// The optional `[start [end]]` arguments at `args[at..]` for a sequence of
/// length `len`, checked: 0 <= start <= end <= len.
pub fn range_args(args: &[Value], at: usize, len: usize, who: &str) -> Result<(usize, usize), Error> {
    let start = args.get(at).map_or(Ok(0), |&v| index_arg(v, who))?;
    let end = args.get(at + 1).map_or(Ok(len), |&v| index_arg(v, who))?;
    if start > end || end > len {
        return Err(err!("{}: range {}..{} is out of bounds for length {}", who, start, end, len));
    }
    Ok((start, end))
}

/// A count or index as a fixnum.
pub fn fixnum(n: usize) -> Value {
    Value::fixnum(n as i64).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every primitive, called with every number of arguments its arity
    /// allows (up to 4) and arguments of every type, returns a value or an
    /// error: it never panics. This checks the argument handling of a few
    /// hundred procedures ported from s1, which did its own arity checks.
    #[test]
    fn no_primitive_panics_on_bad_arguments() {
        let mut heap = Heap::new();
        let core = Env::with_core_forms(&mut heap);
        let system = heap.environment(core);
        install(&mut heap, &mut crate::globals::env_of(system).borrow_mut());
        heap.system = Some(system);
        heap.interaction = Some(system);
        let samples = [
            Value::fixnum(-1).unwrap(),
            Value::fixnum(3).unwrap(),
            Value::flonum(f64::NAN),
            Value::flonum(2.5),
            heap.integer(num_bigint::BigInt::from(1u64 << 62)),
            Value::char('x'),
            Value::TRUE,
            Value::NIL,
            Value::EOF,
            // A string that is also a harmless file name, since
            // open-output-file and delete-file get it too.
            heap.string("target/s2-builtin-test-sample"),
            heap.intern("sym"),
            heap.list(vec![Value::fixnum(1).unwrap(), Value::fixnum(2).unwrap()]),
            heap.vector(vec![Value::char('a')]),
            heap.bytevector(vec![1, 2]),
        ];
        // String ports as the current ports, so nothing reads the terminal
        // or writes to it.
        let mut vm = Vm::new(&mut heap);
        vm.ports[port::INPUT] = heap.port(crate::io::new_string_port_input("(a) b"));
        vm.ports[port::OUTPUT] = heap.port(crate::io::PortKind::StringPortOutput { content: String::new() });
        vm.ports[port::ERROR] = vm.ports[port::OUTPUT];
        for table in [control::PRIMITIVES, number::PRIMITIVES, predicate::PRIMITIVES, list::PRIMITIVES, char::PRIMITIVES, string::PRIMITIVES, vector::PRIMITIVES, record::PRIMITIVES, port::PRIMITIVES] {
            for p in table {
                let max = p.max.unwrap_or(4).min(4);
                for n in p.min..=max {
                    // Rotate through the samples so each position sees each type.
                    for start in 0..samples.len() {
                        let args: Vec<Value> = (0..n).map(|i| samples[(start + i * 5) % samples.len()]).collect();
                        let _ = match p.f {
                            PrimFn::Builtin(f) => f(&mut heap, &args),
                            PrimFn::Vm(f) => f(&mut vm, &mut heap, &args),
                            PrimFn::Apply | PrimFn::CallCc | PrimFn::CallEc | PrimFn::Eval => continue,
                        };
                    }
                }
            }
        }
    }
}
