//! External representations of Scheme values: what `display`, `write`,
//! `write-shared` and `write-simple` produce, and what the REPL prints.
//!
//! Pairs and vectors that close a cycle (or, for `write-shared`, appear
//! more than once) are printed with datum labels (`#0=` / `#0#`), so
//! printing terminates on circular data. Long lists are printed without
//! recursion on the cdr.

use crate::gc::SchemeValue::*;
use crate::gc::{Callable, GcRef};
use crate::gc_value;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// The external representation `display` produces: strings and characters
/// appear as their raw text, at any depth. Cycles get datum labels.
pub fn display_value(obj: &GcRef) -> String {
    render(*obj, false, Labels::Cycles)
}

/// The external representation `write` produces: strings and characters in
/// the escaped form the reader accepts. Cycles get datum labels
/// (`#0=(1 . #0#)`), so printing always terminates.
pub fn print_value(obj: &GcRef) -> String {
    render(*obj, true, Labels::Cycles)
}

/// `print_value`, cut to at most `max` characters (ending in `...`), for
/// debugger output, where one large datum would bury everything else.
pub fn print_value_limited(obj: &GcRef, max: usize) -> String {
    let mut s = print_value(obj);
    if let Some((cut, _)) = s.char_indices().nth(max.saturating_sub(3)) {
        if s.len() > max {
            s.truncate(cut);
            s.push_str("...");
        }
    }
    s
}

/// `write-shared`: datum labels for every pair or vector that appears more
/// than once, cyclic or not.
pub fn write_shared_value(obj: &GcRef) -> String {
    render(*obj, true, Labels::Shared)
}

/// `write-simple`: no datum labels (loops forever on cyclic data).
pub fn write_simple_value(obj: &GcRef) -> String {
    render(*obj, true, Labels::None)
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
fn render(obj: GcRef, write: bool, mode: Labels) -> String {
    let needs = match mode {
        Labels::None => HashSet::default(),
        _ if !is_compound(obj) => HashSet::default(),
        Labels::Cycles => cycle_targets(obj),
        Labels::Shared => shared_nodes(obj),
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

/// Whether `obj` can be part of a cycle: a pair or vector.
fn is_compound(obj: GcRef) -> bool {
    matches!(gc_value!(obj), Pair(..) | Vector(_))
}

/// The children a label search follows.
fn children(obj: GcRef) -> Vec<GcRef> {
    match gc_value!(obj) {
        Pair(car, cdr) => vec![*car, *cdr],
        Vector(items) => items.clone(),
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
                if !is_compound(node) || done.contains(&node) {
                    continue;
                }
                if on_path.contains(&node) {
                    targets.insert(node);
                    continue;
                }
                on_path.insert(node);
                stack.push(Step::Exit(node));
                for child in children(node).into_iter().rev() {
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
        if !is_compound(node) {
            continue;
        }
        if !seen.insert(node) {
            shared.insert(node);
            continue;
        }
        stack.extend(children(node));
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

/// Append the representation of `obj` to `out`.
fn print_into(out: &mut String, obj: GcRef, ctx: &mut Ctx) {
    let write = ctx.write;
    match gc_value!(obj) {
        Pair(_, _) => {
            if label_prefix(out, obj, ctx) {
                return;
            }
            out.push('(');
            let mut current = obj;
            let mut first = true;
            loop {
                match gc_value!(current) {
                    // A labelled tail is printed in dotted form, so its label
                    // (or reference) can appear.
                    Pair(..) if !first && ctx.needs.contains(&current) => {
                        out.push_str(" . ");
                        print_into(out, current, ctx);
                        break;
                    }
                    Pair(car, cdr) => {
                        if !first {
                            out.push(' ');
                        }
                        print_into(out, *car, ctx);
                        current = *cdr;
                        first = false;
                    }
                    Nil => break,
                    _ => {
                        out.push_str(" . ");
                        print_into(out, current, ctx);
                        break;
                    }
                }
            }
            out.push(')');
        }
        Vector(v) => {
            if label_prefix(out, obj, ctx) {
                return;
            }
            out.push_str("#(");
            print_separated(out, v, ctx);
            out.push(')');
        }
        Bytevector(b) => {
            out.push_str("#u8(");
            for (i, byte) in b.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&byte.to_string());
            }
            out.push(')');
        }
        // The values of a `(values ...)` package that reached a printer.
        Values(v) => print_separated(out, v, ctx),
        Symbol(s) if write => write_symbol(out, s),
        Symbol(s) => out.push_str(s),
        Int(i) => out.push_str(&i.to_string()),
        Rational(r) => out.push_str(&format!("{}/{}", r.numer(), r.denom())),
        Float(f) => out.push_str(&format_float(*f)),
        Str(s) if write => write_string(out, s),
        Str(s) => out.push_str(s),
        Char(c) if write => write_char(out, *c),
        Char(c) => out.push(*c),
        Bool(true) => out.push_str("#t"),
        Bool(false) => out.push_str("#f"),
        Nil => out.push_str("()"),
        Void => {}
        Undefined => out.push_str("#<undefined>"),
        Eof => out.push_str("#<eof>"),
        // Procedures and syntax have no external representation (R7RS), so
        // they print as opaque #<...> forms; `procedure-source` gives a
        // closure's source as data.
        Callable(variant) => out.push_str(&match &**variant {
            Callable::Builtin { name, .. } | Callable::SysBuiltin { name, .. } => format!("#<procedure {}>", name),
            Callable::SpecialForm { name, .. } => format!("#<syntax {}>", name),
            Callable::Closure { name, .. } | Callable::CaseLambda { name, .. } => opaque("procedure", name),
            Callable::Macro { name, .. } => opaque("macro", name),
            Callable::SyntaxRules(_) => "#<syntax-rules>".to_string(),
        }),
        Port(port) => out.push_str(&describe_port(port)),
        Continuation(_) => out.push_str("#<continuation>"),
        Environment(_) => out.push_str("#<environment>"),
        RecordType(t) => {
            out.push_str("#<record-type ");
            print_into(out, t.name, ctx);
            out.push('>');
        }
        Record(r) => {
            out.push_str("#<");
            if let RecordType(t) = gc_value!(r.rtype) {
                print_into(out, t.name, ctx);
            }
            for f in &r.fields {
                out.push(' ');
                print_into(out, *f, ctx);
            }
            out.push('>');
        }
        ErrorObject(e) => {
            out.push_str("#<error ");
            let saved = ctx.write;
            ctx.write = true;
            print_into(out, e.message, ctx);
            let mut rest = e.irritants;
            while let Pair(car, cdr) = gc_value!(rest) {
                out.push(' ');
                print_into(out, *car, ctx);
                rest = *cdr;
            }
            ctx.write = saved;
            out.push('>');
        }
        TailCallScheduled => out.push_str("print_value: unprintable."),
    }
}

/// Print `items` separated by spaces.
fn print_separated(out: &mut String, items: &[GcRef], ctx: &mut Ctx) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        print_into(out, *item, ctx);
    }
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
        || crate::number_syntax::parse_number(s, 10) != crate::number_syntax::NumberSyntax::NotANumber
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

/// `#<kind name>`, or `#<kind>` for an anonymous object.
fn opaque(kind: &str, name: &Option<String>) -> String {
    match name {
        Some(name) => format!("#<{} {}>", kind, name),
        None => format!("#<{}>", kind),
    }
}

/// The printed form of a port, e.g. `#<input-port stdin>`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{RunTime, RunTimeStruct};
    use crate::gc::*;
    use num_bigint::BigInt;

    #[test]
    fn test_print_value_basic_types() {
        let mut ev = RunTimeStruct::new();
        let mut ec = RunTime::from_eval(&mut ev);
        let heap = &mut ec.heap;

        assert_eq!(print_value(&new_int(heap, BigInt::from(123))), "123");
        assert_eq!(print_value(&new_float(heap, 3.14)), "3.14");
        assert_eq!(print_value(&new_bool(heap, true)), "#t");
        assert_eq!(print_value(&new_bool(heap, false)), "#f");
        assert_eq!(print_value(&new_string(heap, "hello")), "\"hello\"");
        assert_eq!(print_value(&new_char(heap, 'a')), "#\\a");
        assert_eq!(print_value(&new_char(heap, '\n')), "#\\newline");
        assert_eq!(print_value(&heap.intern_symbol("foo")), "foo");
        assert_eq!(print_value(&heap.nil_s()), "()");
        assert_eq!(print_value(&heap.void()), "");
        assert_eq!(print_value(&heap.unspecified()), "#<undefined>");
        assert_eq!(print_value(&heap.eof()), "#<eof>");
    }

    #[test]
    fn test_write_escapes_and_float_format() {
        let mut ev = RunTimeStruct::new();
        let mut ec = RunTime::from_eval(&mut ev);
        let heap = &mut ec.heap;

        // Strings: quote, backslash and control characters are escaped.
        let s = new_string(heap, "a\"b\\c\nd\te\u{1}");
        assert_eq!(print_value(&s), r#""a\"b\\c\nd\te\x1;""#);
        assert_eq!(display_value(&s), "a\"b\\c\nd\te\u{1}");

        // Characters use R7RS names.
        assert_eq!(print_value(&new_char(heap, ' ')), "#\\space");
        assert_eq!(print_value(&new_char(heap, '\u{7}')), "#\\alarm");
        assert_eq!(print_value(&new_char(heap, '\0')), "#\\null");
        assert_eq!(print_value(&new_char(heap, '\u{1}')), "#\\x1");

        // Flonums always read back as inexact.
        assert_eq!(print_value(&new_float(heap, 2.0)), "2.0");
        assert_eq!(print_value(&new_float(heap, -0.0)), "-0.0");
        assert_eq!(print_value(&new_float(heap, 1e21)), "1.0e+21");
        assert_eq!(print_value(&new_float(heap, 5e-324)), "5.0e-324");
        assert_eq!(print_value(&new_float(heap, 1.7976931348623157e308)), "1.7976931348623157e+308");
        assert_eq!(print_value(&new_float(heap, f64::INFINITY)), "+inf.0");
        assert_eq!(print_value(&new_float(heap, f64::NEG_INFINITY)), "-inf.0");
        assert_eq!(print_value(&new_float(heap, f64::NAN)), "+nan.0");

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
            let sym = heap.intern_symbol(name);
            assert_eq!(print_value(&sym), written, "symbol {:?}", name);
            assert_eq!(display_value(&sym), name);
        }

        // display reaches strings and characters nested in a list.
        let c = new_char(heap, 'x');
        let str_in_list = new_string(heap, "s");
        let tail = new_pair(heap, c, heap.nil_s());
        let list = new_pair(heap, str_in_list, tail);
        assert_eq!(display_value(&list), "(s x)");
        assert_eq!(print_value(&list), "(\"s\" #\\x)");
    }

    #[test]
    fn test_print_value_compound_types() {
        let mut ev = RunTimeStruct::new();
        let mut ec = RunTime::from_eval(&mut ev);
        let heap = &mut ec.heap;

        // Proper List
        let val1 = new_int(heap, BigInt::from(1));
        let val2 = new_int(heap, BigInt::from(2));
        let val3 = new_int(heap, BigInt::from(3));
        let l1 = new_pair(heap, val3, heap.nil_s());
        let l2 = new_pair(heap, val2, l1);
        let list = new_pair(heap, val1, l2);
        assert_eq!(print_value(&list), "(1 2 3)");

        // Dotted List
        let val1_dotted = new_int(heap, BigInt::from(1));
        let val2_dotted = new_int(heap, BigInt::from(2));
        let dotted_list = new_pair(heap, val1_dotted, val2_dotted);
        assert_eq!(print_value(&dotted_list), "(1 . 2)");

        // Vector
        let vec_val1 = new_int(heap, BigInt::from(1));
        let vec_val2 = new_bool(heap, true);
        let vec_val3 = new_string(heap, "foo");
        let vector = new_vector(heap, vec![vec_val1, vec_val2, vec_val3]);
        assert_eq!(print_value(&vector), "#(1 #t \"foo\")");
    }

    #[test]
    fn test_display_value_basic_types() {
        let mut ev = RunTimeStruct::new();
        let mut ec = RunTime::from_eval(&mut ev);
        let heap = &mut ec.heap;

        assert_eq!(display_value(&new_int(heap, BigInt::from(123))), "123");
        assert_eq!(display_value(&new_float(heap, 3.14)), "3.14");
        assert_eq!(display_value(&new_bool(heap, true)), "#t");
        assert_eq!(display_value(&new_bool(heap, false)), "#f");
        assert_eq!(display_value(&new_string(heap, "hello")), "hello"); // No quotes
        assert_eq!(display_value(&new_char(heap, 'a')), "a"); // No #\ prefix
        assert_eq!(display_value(&new_char(heap, '\n')), "\n");
        assert_eq!(display_value(&heap.intern_symbol("foo")), "foo");
        assert_eq!(display_value(&heap.nil_s()), "()");
        assert_eq!(display_value(&heap.void()), "");
        assert_eq!(display_value(&heap.unspecified()), "#<undefined>"); // Corrected
        assert_eq!(display_value(&heap.eof()), "#<eof>"); // Corrected
    }

    #[test]
    fn test_display_value_compound_types() {
        let mut ev = RunTimeStruct::new();
        let mut ec = RunTime::from_eval(&mut ev);
        let heap = &mut ec.heap;

        // Proper List
        let val1 = new_int(heap, BigInt::from(1));
        let val2 = new_int(heap, BigInt::from(2));
        let val3 = new_int(heap, BigInt::from(3));
        let l1 = new_pair(heap, val3, heap.nil_s());
        let l2 = new_pair(heap, val2, l1);
        let list = new_pair(heap, val1, l2);
        assert_eq!(display_value(&list), "(1 2 3)");

        // Dotted List
        let val1_dotted = new_int(heap, BigInt::from(1));
        let val2_dotted = new_int(heap, BigInt::from(2));
        let dotted_list = new_pair(heap, val1_dotted, val2_dotted);
        assert_eq!(display_value(&dotted_list), "(1 . 2)");

        // Vector
        let vec_val1 = new_int(heap, BigInt::from(1));
        let vec_val2 = new_bool(heap, true);
        let vec_val3 = new_string(heap, "foo");
        let vector = new_vector(heap, vec![vec_val1, vec_val2, vec_val3]);
        // display applies at every depth, so the nested string is unquoted.
        assert_eq!(display_value(&vector), "#(1 #t foo)");
    }
}
