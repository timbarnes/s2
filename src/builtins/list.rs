//! Pairs and lists (R7RS §6.4), forked from s1's `builtin/list.rs`. `map`,
//! `for-each`, `member` and `assoc`, which call procedures, are in
//! `scheme/core.scm`.

use super::{Primitive, expected, fixnum, index_arg, kind, list_arg, pair_arg, prim};
use crate::heap::{Heap, Obj, eqv};
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("pair?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Pair { .. }))))),
    prim("null?", 1, Some(1), |_, a| Ok(Value::bool(a[0].is_nil()))),
    prim("list?", 1, Some(1), |_, a| Ok(Value::bool(proper_length(a[0]).is_some()))),
    prim("cons", 2, Some(2), |h, a| Ok(h.cons(a[0], a[1]))),
    prim("car", 1, Some(1), |_, a| Ok(pair_arg(a[0], "car")?.0.get())),
    prim("cdr", 1, Some(1), |_, a| Ok(pair_arg(a[0], "cdr")?.1.get())),
    prim("set-car!", 2, Some(2), |_, a| {
        pair_arg(a[0], "set-car!")?.0.set(a[1]);
        Ok(Value::UNSPECIFIED)
    }),
    prim("set-cdr!", 2, Some(2), |_, a| {
        pair_arg(a[0], "set-cdr!")?.1.set(a[1]);
        Ok(Value::UNSPECIFIED)
    }),
    prim("caar", 1, Some(1), |_, a| cxr(a[0], "caar")),
    prim("cadr", 1, Some(1), |_, a| cxr(a[0], "cadr")),
    prim("cdar", 1, Some(1), |_, a| cxr(a[0], "cdar")),
    prim("cddr", 1, Some(1), |_, a| cxr(a[0], "cddr")),
    prim("caaar", 1, Some(1), |_, a| cxr(a[0], "caaar")),
    prim("caadr", 1, Some(1), |_, a| cxr(a[0], "caadr")),
    prim("cadar", 1, Some(1), |_, a| cxr(a[0], "cadar")),
    prim("caddr", 1, Some(1), |_, a| cxr(a[0], "caddr")),
    prim("cdaar", 1, Some(1), |_, a| cxr(a[0], "cdaar")),
    prim("cdadr", 1, Some(1), |_, a| cxr(a[0], "cdadr")),
    prim("cddar", 1, Some(1), |_, a| cxr(a[0], "cddar")),
    prim("cdddr", 1, Some(1), |_, a| cxr(a[0], "cdddr")),
    prim("caaaar", 1, Some(1), |_, a| cxr(a[0], "caaaar")),
    prim("caaadr", 1, Some(1), |_, a| cxr(a[0], "caaadr")),
    prim("caadar", 1, Some(1), |_, a| cxr(a[0], "caadar")),
    prim("caaddr", 1, Some(1), |_, a| cxr(a[0], "caaddr")),
    prim("cadaar", 1, Some(1), |_, a| cxr(a[0], "cadaar")),
    prim("cadadr", 1, Some(1), |_, a| cxr(a[0], "cadadr")),
    prim("caddar", 1, Some(1), |_, a| cxr(a[0], "caddar")),
    prim("cadddr", 1, Some(1), |_, a| cxr(a[0], "cadddr")),
    prim("cdaaar", 1, Some(1), |_, a| cxr(a[0], "cdaaar")),
    prim("cdaadr", 1, Some(1), |_, a| cxr(a[0], "cdaadr")),
    prim("cdadar", 1, Some(1), |_, a| cxr(a[0], "cdadar")),
    prim("cdaddr", 1, Some(1), |_, a| cxr(a[0], "cdaddr")),
    prim("cddaar", 1, Some(1), |_, a| cxr(a[0], "cddaar")),
    prim("cddadr", 1, Some(1), |_, a| cxr(a[0], "cddadr")),
    prim("cdddar", 1, Some(1), |_, a| cxr(a[0], "cdddar")),
    prim("cddddr", 1, Some(1), |_, a| cxr(a[0], "cddddr")),
    prim("list", 0, None, |h, a| Ok(h.list(a.to_vec()))),
    prim("make-list", 1, Some(2), |h, a| {
        let n = index_arg(a[0], "make-list")?;
        Ok(h.list(vec![a.get(1).copied().unwrap_or(Value::UNSPECIFIED); n]))
    }),
    prim("length", 1, Some(1), |_, a| {
        proper_length(a[0]).map(fixnum).ok_or_else(|| "length: argument must be a proper list".into())
    }),
    prim("append", 0, None, append),
    prim("reverse", 1, Some(1), |h, a| {
        let items = list_arg(a[0], "reverse")?;
        Ok(items.into_iter().fold(Value::NIL, |tail, v| h.cons(v, tail)))
    }),
    prim("list-tail", 2, Some(2), |_, a| list_tail(a[0], a[1], "list-tail")),
    prim("list-ref", 2, Some(2), |_, a| Ok(pair_arg(list_tail(a[0], a[1], "list-ref")?, "list-ref")?.0.get())),
    prim("list-set!", 3, Some(3), |_, a| {
        pair_arg(list_tail(a[0], a[1], "list-set!")?, "list-set!")?.0.set(a[2]);
        Ok(Value::UNSPECIFIED)
    }),
    prim("list-copy", 1, Some(1), list_copy),
    prim("memq", 2, Some(2), |_, a| mem_by(a, "memq", Value::eq)),
    prim("memv", 2, Some(2), |_, a| mem_by(a, "memv", eqv)),
    prim("assq", 2, Some(2), |_, a| ass_by(a, "assq", Value::eq)),
    prim("assv", 2, Some(2), |_, a| ass_by(a, "assv", eqv)),
];

/// `caar` … `cddddr`: the letters between `c` and `r`, applied right to
/// left.
fn cxr(v: Value, name: &str) -> Result<Value, String> {
    let mut v = v;
    for op in name[1..name.len() - 1].chars().rev() {
        let (car, cdr) = pair_arg(v, name)?;
        v = if op == 'a' { car.get() } else { cdr.get() };
    }
    Ok(v)
}

/// The length of a proper list; None for an improper or circular one.
/// Circularity is found by a second pointer moving at half speed.
fn proper_length(v: Value) -> Option<usize> {
    let (mut fast, mut slow) = (v, v);
    let mut n = 0;
    loop {
        if fast.is_nil() {
            return Some(n);
        }
        fast = pair_arg(fast, "").ok()?.1.get();
        n += 1;
        if n % 2 == 0 {
            slow = pair_arg(slow, "").ok()?.1.get();
            if fast.eq(slow) && !fast.is_nil() {
                return None;
            }
        }
    }
}

/// The lists appended; the last argument is shared, not copied, and may be
/// any value.
fn append(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let Some((&last, init)) = args.split_last() else { return Ok(Value::NIL) };
    let mut items = Vec::new();
    for &list in init {
        items.extend(list_arg(list, "append")?);
    }
    Ok(items.into_iter().rev().fold(last, |tail, v| heap.cons(v, tail)))
}

fn list_tail(list: Value, k: Value, who: &str) -> Result<Value, String> {
    let mut v = list;
    for _ in 0..super::index_arg(k, who)? {
        v = pair_arg(v, who).map_err(|_| format!("{}: index out of range", who))?.1.get();
    }
    Ok(v)
}

/// A copy of the pairs of a list; the final cdr (and a non-list) is shared.
fn list_copy(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let mut items = Vec::new();
    let mut v = args[0];
    while let Ok((car, cdr)) = pair_arg(v, "") {
        items.push(car.get());
        v = cdr.get();
    }
    Ok(items.into_iter().rev().fold(v, |tail, x| heap.cons(x, tail)))
}

fn mem_by(args: &[Value], who: &str, same: fn(Value, Value) -> bool) -> Result<Value, String> {
    let mut list = args[1];
    while !list.is_nil() {
        let (car, cdr) = pair_arg(list, who).map_err(|_| expected(who, "a list", args[1]))?;
        if same(args[0], car.get()) {
            return Ok(list);
        }
        list = cdr.get();
    }
    Ok(Value::FALSE)
}

fn ass_by(args: &[Value], who: &str, same: fn(Value, Value) -> bool) -> Result<Value, String> {
    let mut list = args[1];
    while !list.is_nil() {
        let (car, cdr) = pair_arg(list, who).map_err(|_| expected(who, "a list", args[1]))?;
        let entry = car.get();
        if same(args[0], pair_arg(entry, who)?.0.get()) {
            return Ok(entry);
        }
        list = cdr.get();
    }
    Ok(Value::FALSE)
}
