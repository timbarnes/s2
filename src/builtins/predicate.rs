//! Equivalence predicates (R7RS §6.1), booleans (§6.3) and symbols (§6.5).
//! Forked from s1's `builtin/predicate.rs`, without its extensions
//! (`type-of`, `procedure-source`).

use super::{Primitive, expected, kind, prim, string_arg};
use crate::heap::{Heap, Obj, eqv, equal};
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("eq?", 2, Some(2), |_, a| Ok(Value::bool(a[0].eq(a[1])))),
    prim("eqv?", 2, Some(2), |_, a| Ok(Value::bool(eqv(a[0], a[1])))),
    prim("equal?", 2, Some(2), |_, a| Ok(Value::bool(equal(a[0], a[1])))),
    prim("not", 1, Some(1), |_, a| Ok(Value::bool(!a[0].is_true()))),
    prim("boolean?", 1, Some(1), |_, a| Ok(Value::bool(a[0].as_bool().is_some()))),
    prim("boolean=?", 2, None, |_, a| all_same(a, "boolean=?", "a boolean", |v| v.as_bool().is_some())),
    prim("symbol?", 1, Some(1), |_, a| Ok(Value::bool(is_symbol(a[0])))),
    prim("symbol=?", 2, None, |_, a| all_same(a, "symbol=?", "a symbol", is_symbol)),
    prim("symbol->string", 1, Some(1), symbol_to_string),
    prim("string->symbol", 1, Some(1), |h, a| {
        let name = string_arg(a[0], "string->symbol")?.borrow().to_string();
        Ok(h.intern(&name))
    }),
];

fn is_symbol(v: Value) -> bool {
    matches!(kind(v), Some(Obj::Symbol(_)))
}

/// `boolean=?`, `symbol=?`: every argument has the type, and all are `eq?`.
fn all_same(args: &[Value], who: &str, what: &str, is: fn(Value) -> bool) -> Result<Value, String> {
    if let Some(&bad) = args.iter().find(|&&a| !is(a)) {
        return Err(expected(who, what, bad));
    }
    Ok(Value::bool(args.windows(2).all(|w| w[0].eq(w[1]))))
}

fn symbol_to_string(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    match kind(args[0]) {
        Some(Obj::Symbol(name)) => Ok(heap.string(name)),
        _ => Err(expected("symbol->string", "a symbol", args[0])),
    }
}
