//! External representations of Scheme values: what `display`, `write`,
//! `write-shared` and `write-simple` produce, and what the REPL prints.
//!
//! Pairs and vectors that close a cycle (or, for `write-shared`, appear
//! more than once) are printed with datum labels (`#0=` / `#0#`), so
//! printing terminates on circular data. Long lists are printed without
//! recursion on the cdr.
//!
//! Forked from s1/src/printer.rs at bc6f118, adapted to `Value`. P0 prints
//! the kinds of object the reader makes; procedures, ports, records and so
//! on come back with the phases that add them.

use crate::heap::{GcRef, Obj};
use crate::value::Value;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// The external representation `display` produces: strings and characters
/// appear as their raw text, at any depth. Cycles get datum labels.
pub fn display_value(obj: Value) -> String {
    render(obj, false, Labels::Cycles)
}

/// The external representation `write` produces: strings and characters in
/// the escaped form the reader accepts. Cycles get datum labels
/// (`#0=(1 . #0#)`), so printing always terminates.
pub fn print_value(obj: Value) -> String {
    render(obj, true, Labels::Cycles)
}

/// `write-shared`: datum labels for every pair or vector that appears more
/// than once, cyclic or not.
pub fn write_shared_value(obj: Value) -> String {
    render(obj, true, Labels::Shared)
}

/// `write-simple`: no datum labels (loops forever on cyclic data).
pub fn write_simple_value(obj: Value) -> String {
    render(obj, true, Labels::None)
}

#[derive(Clone, Copy, PartialEq)]
/// Which objects get datum labels.
enum Labels {
    /// Only those that close a cycle (`write`, `display`).
    Cycles,
    /// Every pair or vector reached more than once (`write-shared`).
    Shared,
    /// None (`write-simple`).
    None,
}

/// Datum-label state while printing one object.
struct Ctx {
    /// `write` style (escaped strings and characters) rather than
    /// `display` style.
    write: bool,
    /// Objects that get a label
    needs: HashSet<GcRef>,
    /// Labels already printed
    assigned: HashMap<GcRef, usize>,
}

/// Print `obj` in `write` or `display` style, labelling per `mode`.
fn render(obj: Value, write: bool, mode: Labels) -> String {
    let needs = match (mode, compound(obj)) {
        (Labels::None, _) | (_, None) => HashSet::default(),
        (Labels::Cycles, Some(r)) => cycle_targets(r),
        (Labels::Shared, Some(r)) => shared_nodes(r),
    };
    let mut ctx = Ctx {
        write,
        needs,
        assigned: HashMap::default(),
    };
    let mut out = String::new();
    print_into(&mut out, obj, &mut ctx);
    out
}

/// The object, if `v` can be part of a cycle: a pair or vector.
fn compound(v: Value) -> Option<GcRef> {
    let r = v.as_ref()?;
    matches!(r.get().kind, Obj::Pair { .. } | Obj::Vector(_)).then_some(r)
}

/// The children a label search follows.
fn children(obj: GcRef) -> Vec<Value> {
    match &obj.get().kind {
        Obj::Pair { car, cdr } => vec![car.get(), cdr.get()],
        Obj::Vector(items) => items.iter().map(|c| c.get()).collect(),
        _ => Vec::new(),
    }
}

/// Pairs and vectors that close a cycle: the targets of back edges in a
/// depth-first walk (every cycle has one). Iterative, so long lists don't
/// exhaust the stack.
fn cycle_targets(root: GcRef) -> HashSet<GcRef> {
    enum Step {
        Enter(GcRef),
        Exit(GcRef),
    }
    let mut on_path = HashSet::default();
    let mut done = HashSet::default();
    let mut targets = HashSet::default();
    let mut stack = vec![Step::Enter(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(node) => {
                if done.contains(&node) {
                    continue;
                }
                if on_path.contains(&node) {
                    targets.insert(node);
                    continue;
                }
                on_path.insert(node);
                stack.push(Step::Exit(node));
                for child in children(node).into_iter().rev().filter_map(compound) {
                    stack.push(Step::Enter(child));
                }
            }
            Step::Exit(node) => {
                on_path.remove(&node);
                done.insert(node);
            }
        }
    }
    targets
}

/// Pairs and vectors reachable more than once.
fn shared_nodes(root: GcRef) -> HashSet<GcRef> {
    let mut seen = HashSet::default();
    let mut shared = HashSet::default();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            shared.insert(node);
            continue;
        }
        stack.extend(children(node).into_iter().filter_map(compound));
    }
    shared
}

/// Print `obj` as `#n#` if its label was already printed; otherwise print
/// `#n=` first if it needs a label. Returns true if it printed a reference
/// (and nothing more should be printed for `obj`).
fn label_prefix(out: &mut String, obj: GcRef, ctx: &mut Ctx) -> bool {
    if !ctx.needs.contains(&obj) {
        return false;
    }
    if let Some(n) = ctx.assigned.get(&obj) {
        out.push_str(&format!("#{}#", n));
        return true;
    }
    let n = ctx.assigned.len();
    ctx.assigned.insert(obj, n);
    out.push_str(&format!("#{}=", n));
    false
}

/// Append the representation of `v` to `out`.
fn print_into(out: &mut String, v: Value, ctx: &mut Ctx) {
    let write = ctx.write;
    let Some(obj) = v.as_ref() else {
        return print_immediate(out, v, write);
    };
    match &obj.get().kind {
        Obj::Pair { .. } => {
            if label_prefix(out, obj, ctx) {
                return;
            }
            out.push('(');
            let mut current = v;
            let mut first = true;
            loop {
                match current.as_ref().map(|r| (r, &r.get().kind)) {
                    // A labelled tail is printed in dotted form, so its label
                    // (or reference) can appear.
                    Some((r, Obj::Pair { .. })) if !first && ctx.needs.contains(&r) => {
                        out.push_str(" . ");
                        print_into(out, current, ctx);
                        break;
                    }
                    Some((_, Obj::Pair { car, cdr })) => {
                        if !first {
                            out.push(' ');
                        }
                        print_into(out, car.get(), ctx);
                        current = cdr.get();
                        first = false;
                    }
                    _ if current.is_nil() => break,
                    _ => {
                        out.push_str(" . ");
                        print_into(out, current, ctx);
                        break;
                    }
                }
            }
            out.push(')');
        }
        Obj::Vector(items) => {
            if label_prefix(out, obj, ctx) {
                return;
            }
            out.push_str("#(");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                print_into(out, item.get(), ctx);
            }
            out.push(')');
        }
        Obj::Bytevector(b) => {
            out.push_str("#u8(");
            for (i, byte) in b.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&byte.get().to_string());
            }
            out.push(')');
        }
        Obj::Symbol(s) if write => write_symbol(out, s),
        Obj::Symbol(s) => out.push_str(s),
        Obj::BigInt(i) => out.push_str(&i.to_string()),
        Obj::Rational(r) => out.push_str(&format!("{}/{}", r.numer(), r.denom())),
        Obj::Str(s) if write => write_string(out, &s.borrow()),
        Obj::Str(s) => out.push_str(&s.borrow()),
        Obj::Global { name, .. } => {
            out.push_str("#<global ");
            print_into(out, Value::obj(*name), ctx);
            out.push('>');
        }
        Obj::Primitive(p) => out.push_str(&format!("#<procedure {}>", p.name)),
        Obj::Closure { template, .. } => match &template.get().kind {
            Obj::Template(t) if t.name.is_empty() => out.push_str("#<procedure>"),
            Obj::Template(t) => out.push_str(&format!("#<procedure {}>", t.name)),
            _ => unreachable!(),
        },
        Obj::Box(_) => out.push_str("#<box>"),
        Obj::Environment(_) => out.push_str("#<environment>"),
        Obj::Continuation { .. } | Obj::Escape { .. } => out.push_str("#<continuation>"),
        // Several values print separated by spaces; the REPL prints them
        // one per line instead.
        Obj::Values(items) => {
            for (i, &v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                print_into(out, v, ctx);
            }
        }
        Obj::Error { message, irritants, .. } => {
            out.push_str("#<error ");
            let saved = ctx.write;
            ctx.write = true;
            print_into(out, *message, ctx);
            let mut rest = *irritants;
            while let Some(Obj::Pair { car, cdr }) = rest.as_obj().map(|o| &o.kind) {
                out.push(' ');
                print_into(out, car.get(), ctx);
                rest = cdr.get();
            }
            ctx.write = saved;
            out.push('>');
        }
        Obj::Port(p) => out.push_str(&describe_port(&p.borrow())),
        Obj::RecordType { name, .. } => {
            out.push_str("#<record-type ");
            print_into(out, *name, ctx);
            out.push('>');
        }
        Obj::Record { rtype, fields } => {
            out.push_str("#<");
            if let Obj::RecordType { name, .. } = &rtype.get().kind {
                print_into(out, *name, ctx);
            }
            for f in fields.iter() {
                out.push(' ');
                print_into(out, f.get(), ctx);
            }
            out.push('>');
        }
        Obj::Alias { .. } => print_into(out, Value::obj(crate::compile::syntax_rules::strip(obj)), ctx),
        Obj::Macro(_) => out.push_str("#<macro>"),
        Obj::Template(t) if t.name.is_empty() => out.push_str("#<template>"),
        Obj::Template(t) => out.push_str(&format!("#<template {}>", t.name)),
    }
}

/// The printed form of a port, e.g. `#<input-port stdin>` (as in s1).
fn describe_port(port: &crate::io::PortKind) -> String {
    use crate::io::PortKind::*;
    match port {
        Stdin => "#<input-port stdin>".to_string(),
        Stdout => "#<output-port stdout>".to_string(),
        Stderr => "#<output-port stderr>".to_string(),
        StringPortInput { .. } => "#<input-port string>".to_string(),
        StringPortOutput { .. } => "#<output-port string>".to_string(),
        BytevectorInput { .. } => "#<binary-input-port bytevector>".to_string(),
        BytevectorOutput { .. } => "#<binary-output-port bytevector>".to_string(),
        FileOutput { name, binary: false, .. } => format!("#<output-port {:?}>", name),
        FileOutput { name, binary: true, .. } => format!("#<binary-output-port {:?}>", name),
        Closed { .. } => "#<closed-port>".to_string(),
    }
}

/// Append a value that isn't a heap object.
fn print_immediate(out: &mut String, v: Value, write: bool) {
    if let Some(n) = v.as_fixnum() {
        out.push_str(&n.to_string());
    } else if let Some(f) = v.as_flonum() {
        out.push_str(&format_float(f));
    } else if let Some(c) = v.as_char() {
        if write { write_char(out, c) } else { out.push(c) }
    } else if let Some(b) = v.as_bool() {
        out.push_str(if b { "#t" } else { "#f" });
    } else if v.is_nil() {
        out.push_str("()");
    } else if v.is_eof() {
        out.push_str("#<eof>");
    } else if v.is_undefined() {
        out.push_str("#<undefined>");
    }
    // Unspecified prints as nothing, so the REPL shows nothing for it.
}

/// Format a flonum so it reads back as one: always with a decimal point or
/// exponent (`2.0`, not `2`), and the R7RS spellings of the infinities and
/// NaN. Rust's `Debug` output is already shortest-round-trip and switches
/// to exponent notation for very large and small magnitudes, which is
/// written conventionally: `1.0e+21`, `5.0e-324`.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        "+nan.0".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "+inf.0" } else { "-inf.0" }.to_string()
    } else {
        let s = format!("{:?}", f);
        match s.split_once('e') {
            Some((mantissa, exp)) => {
                let point = if mantissa.contains('.') { "" } else { ".0" };
                let sign = if exp.starts_with('-') { "" } else { "+" };
                format!("{}{}e{}{}", mantissa, point, sign, exp)
            }
            None => s,
        }
    }
}

/// Write a symbol, in `|...|` form when its bare name would read back as
/// something else.
fn write_symbol(out: &mut String, s: &str) {
    if !needs_bars(s) {
        out.push_str(s);
        return;
    }
    out.push('|');
    for c in s.chars() {
        match c {
            '|' => out.push_str("\\|"),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\x{:x};", c as u32)),
            c => out.push(c),
        }
    }
    out.push('|');
}

/// Whether a symbol named `s` must be written as `|...|` to read back
/// as the same symbol.
fn needs_bars(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return true; // the empty symbol
    };
    let second = chars.next();
    let numeric_start = first.is_ascii_digit()
        || (first == '.' && second.is_some_and(|c| c.is_ascii_digit()))
        || (matches!(first, '+' | '-')
            && (second.is_some_and(|c| c.is_ascii_digit() || c == '.')
                || s[1..].to_ascii_lowercase().starts_with("inf")
                || s[1..].to_ascii_lowercase().starts_with("nan")));
    s == "."
        || first == '#'
        || numeric_start
        || crate::reader::number_syntax::parse_number(s, 10) != crate::reader::number_syntax::NumberSyntax::NotANumber
        || s.chars().any(|c| c.is_whitespace() || c.is_control() || "()[]\";'`,|\\".contains(c))
}

/// Write a string literal with the escapes R7RS's reader understands.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            c if c.is_control() => out.push_str(&format!("\\x{:x};", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Write a character literal, using R7RS's character names where they
/// exist and hex escapes for other control characters.
fn write_char(out: &mut String, c: char) {
    out.push_str("#\\");
    match c {
        '\u{7}' => out.push_str("alarm"),
        '\u{8}' => out.push_str("backspace"),
        '\u{7f}' => out.push_str("delete"),
        '\u{1b}' => out.push_str("escape"),
        '\n' => out.push_str("newline"),
        '\0' => out.push_str("null"),
        '\r' => out.push_str("return"),
        ' ' => out.push_str("space"),
        '\t' => out.push_str("tab"),
        c if c.is_control() => out.push_str(&format!("x{:x}", c as u32)),
        c => out.push(c),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::Heap;
    use num_bigint::BigInt;

    fn int(heap: &mut Heap, n: i64) -> Value {
        heap.integer(BigInt::from(n))
    }

    #[test]
    fn basic_types() {
        let mut heap = Heap::new();
        let foo = heap.intern("foo");
        let hello = heap.string("hello");
        for (v, written, displayed) in [
            (int(&mut heap, 123), "123", "123"),
            (Value::flonum(3.14), "3.14", "3.14"),
            (Value::TRUE, "#t", "#t"),
            (Value::FALSE, "#f", "#f"),
            (hello, "\"hello\"", "hello"),
            (Value::char('a'), "#\\a", "a"),
            (Value::char('\n'), "#\\newline", "\n"),
            (foo, "foo", "foo"),
            (Value::NIL, "()", "()"),
            (Value::UNSPECIFIED, "", ""),
            (Value::UNDEFINED, "#<undefined>", "#<undefined>"),
            (Value::EOF, "#<eof>", "#<eof>"),
        ] {
            assert_eq!(print_value(v), written);
            assert_eq!(display_value(v), displayed);
        }
    }

    #[test]
    fn write_escapes_and_float_format() {
        let mut heap = Heap::new();

        // Strings: quote, backslash and control characters are escaped.
        let s = heap.string("a\"b\\c\nd\te\u{1}");
        assert_eq!(print_value(s), r#""a\"b\\c\nd\te\x1;""#);
        assert_eq!(display_value(s), "a\"b\\c\nd\te\u{1}");

        // Characters use R7RS names.
        assert_eq!(print_value(Value::char(' ')), "#\\space");
        assert_eq!(print_value(Value::char('\u{7}')), "#\\alarm");
        assert_eq!(print_value(Value::char('\0')), "#\\null");
        assert_eq!(print_value(Value::char('\u{1}')), "#\\x1");

        // Flonums always read back as inexact.
        for (f, written) in [
            (2.0, "2.0"),
            (-0.0, "-0.0"),
            (1e21, "1.0e+21"),
            (5e-324, "5.0e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (f64::INFINITY, "+inf.0"),
            (f64::NEG_INFINITY, "-inf.0"),
            (f64::NAN, "+nan.0"),
        ] {
            assert_eq!(print_value(Value::flonum(f)), written);
        }

        // Symbols that wouldn't read back bare are written with bars.
        for (name, written) in [
            ("abc", "abc"),
            ("->x", "->x"),
            ("...", "..."),
            ("a b", "|a b|"),
            ("", "||"),
            (".", "|.|"),
            ("2", "|2|"),
            ("+3", "|+3|"),
            ("-.4", "|-.4|"),
            ("+i", "|+i|"),
            ("+NaN.0abc", "|+NaN.0abc|"),
            ("|", "|\\||"),
            ("\\123", "|\\\\123|"),
            ("nil", "nil"),
        ] {
            let sym = heap.intern(name);
            assert_eq!(print_value(sym), written, "symbol {:?}", name);
            assert_eq!(display_value(sym), name);
        }

        // display reaches strings and characters nested in a list.
        let s = heap.string("s");
        let list = heap.list(vec![s, Value::char('x')]);
        assert_eq!(display_value(list), "(s x)");
        assert_eq!(print_value(list), "(\"s\" #\\x)");
    }

    #[test]
    fn compound_types() {
        let mut heap = Heap::new();
        let (one, two, three) = (int(&mut heap, 1), int(&mut heap, 2), int(&mut heap, 3));
        let list = heap.list(vec![one, two, three]);
        assert_eq!(print_value(list), "(1 2 3)");
        let dotted = heap.cons(one, two);
        assert_eq!(print_value(dotted), "(1 . 2)");
        let foo = heap.string("foo");
        let vector = heap.vector(vec![one, Value::TRUE, foo]);
        assert_eq!(print_value(vector), "#(1 #t \"foo\")");
        // display applies at every depth, so the nested string is unquoted.
        assert_eq!(display_value(vector), "#(1 #t foo)");
    }

    #[test]
    fn shared_and_cyclic() {
        let mut heap = Heap::new();
        let a = heap.intern("a");
        let x = heap.list(vec![a]);
        let both = heap.list(vec![x, x]);
        assert_eq!(print_value(both), "((a) (a))");
        assert_eq!(write_shared_value(both), "(#0=(a) #0#)");
        let cell = heap.cons(a, Value::NIL);
        if let Obj::Pair { cdr, .. } = &cell.as_obj().unwrap().kind {
            cdr.set(cell);
        }
        assert_eq!(print_value(cell), "#0=(a . #0#)");
    }
}
