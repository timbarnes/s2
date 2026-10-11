//! Vectors (R7RS §6.8) and bytevectors (§6.9), forked from s1's
//! `builtin/vector.rs` and `builtin/bytevector.rs`.

use std::cell::Cell;

use super::{Error, err, Primitive, bytevector_arg, expected, fixnum, index_arg, kind, list_arg, prim, range_args, string_arg, vector_arg};
use crate::heap::Obj;
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("vector?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Vector(_)))))),
    prim("make-vector", 1, Some(2), |h, a| {
        let n = index_arg(a[0], "make-vector")?;
        Ok(h.vector(vec![a.get(1).copied().unwrap_or(Value::UNSPECIFIED); n]))
    }),
    prim("vector", 0, None, |h, a| Ok(h.vector(a.to_vec()))),
    prim("vector-length", 1, Some(1), |_, a| Ok(fixnum(vector_arg(a[0], "vector-length")?.len()))),
    prim("vector-ref", 2, Some(2), |_, a| Ok(element(vector_arg(a[0], "vector-ref")?, a[1], "vector-ref")?.get())),
    prim("vector-set!", 3, Some(3), |_, a| {
        element(vector_arg(a[0], "vector-set!")?, a[1], "vector-set!")?.set(a[2]);
        Ok(Value::UNSPECIFIED)
    }),
    prim("vector->list", 1, Some(3), |h, a| {
        let items = values(a, "vector->list")?;
        Ok(h.list(items))
    }),
    prim("list->vector", 1, Some(1), |h, a| Ok(h.vector(list_arg(a[0], "list->vector")?))),
    prim("vector-copy", 1, Some(3), |h, a| {
        let items = values(a, "vector-copy")?;
        Ok(h.vector(items))
    }),
    prim("vector-copy!", 3, Some(5), |_, a| {
        // Copy out first: `to` and `from` may be the same vector.
        let src = values(&a[2..], "vector-copy!")?;
        let to = vector_arg(a[0], "vector-copy!")?;
        let at = fits(a[1], src.len(), to.len(), "vector-copy!")?;
        to[at..at + src.len()].iter().zip(src).for_each(|(c, v)| c.set(v));
        Ok(Value::UNSPECIFIED)
    }),
    prim("vector-append", 0, None, |h, a| {
        let mut out = Vec::new();
        for &v in a {
            out.extend(vector_arg(v, "vector-append")?.iter().map(Cell::get));
        }
        Ok(h.vector(out))
    }),
    prim("vector-fill!", 2, Some(4), |_, a| {
        let items = vector_arg(a[0], "vector-fill!")?;
        let (start, end) = range_args(a, 2, items.len(), "vector-fill!")?;
        items[start..end].iter().for_each(|c| c.set(a[1]));
        Ok(Value::UNSPECIFIED)
    }),
    prim("bytevector?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Bytevector(_)))))),
    prim("make-bytevector", 1, Some(2), |h, a| {
        let n = index_arg(a[0], "make-bytevector")?;
        let fill = a.get(1).map_or(Ok(0), |&b| byte(b, "make-bytevector"))?;
        Ok(h.bytevector(vec![fill; n]))
    }),
    prim("bytevector", 0, None, |h, a| {
        let bytes = a.iter().map(|&b| byte(b, "bytevector")).collect::<Result<Vec<u8>, _>>()?;
        Ok(h.bytevector(bytes))
    }),
    prim("bytevector-length", 1, Some(1), |_, a| Ok(fixnum(bytevector_arg(a[0], "bytevector-length")?.len()))),
    prim("bytevector-u8-ref", 2, Some(2), |_, a| {
        let b = bytevector_arg(a[0], "bytevector-u8-ref")?;
        Ok(fixnum(element(b, a[1], "bytevector-u8-ref")?.get() as usize))
    }),
    prim("bytevector-u8-set!", 3, Some(3), |_, a| {
        let b = bytevector_arg(a[0], "bytevector-u8-set!")?;
        element(b, a[1], "bytevector-u8-set!")?.set(byte(a[2], "bytevector-u8-set!")?);
        Ok(Value::UNSPECIFIED)
    }),
    prim("bytevector-copy", 1, Some(3), |h, a| {
        let bytes = bytes(a, "bytevector-copy")?;
        Ok(h.bytevector(bytes))
    }),
    prim("bytevector-copy!", 3, Some(5), |_, a| {
        let src = bytes(&a[2..], "bytevector-copy!")?;
        let to = bytevector_arg(a[0], "bytevector-copy!")?;
        let at = fits(a[1], src.len(), to.len(), "bytevector-copy!")?;
        to[at..at + src.len()].iter().zip(src).for_each(|(c, b)| c.set(b));
        Ok(Value::UNSPECIFIED)
    }),
    prim("bytevector-append", 0, None, |h, a| {
        let mut out = Vec::new();
        for &v in a {
            out.extend(bytevector_arg(v, "bytevector-append")?.iter().map(Cell::get));
        }
        Ok(h.bytevector(out))
    }),
    prim("utf8->string", 1, Some(3), |h, a| {
        let bytes = bytes(a, "utf8->string")?;
        let s = String::from_utf8(bytes).map_err(|_| "utf8->string: invalid UTF-8".to_string())?;
        Ok(h.string(&s))
    }),
    prim("string->utf8", 1, Some(3), |h, a| {
        let chars: Vec<char> = string_arg(a[0], "string->utf8")?.borrow().chars().collect();
        let (start, end) = range_args(a, 1, chars.len(), "string->utf8")?;
        Ok(h.bytevector(chars[start..end].iter().collect::<String>().into_bytes()))
    }),
];

/// Element `k` of `items`, checked.
fn element<'a, T>(items: &'a [Cell<T>], k: Value, who: &str) -> Result<&'a Cell<T>, Error> {
    let k = index_arg(k, who)?;
    items.get(k).ok_or_else(|| err!("{}: index {} out of range for length {}", who, k, items.len()))
}

/// The elements of the vector `args[0]` in the range `args[1..3]`.
fn values(args: &[Value], who: &str) -> Result<Vec<Value>, Error> {
    let items = vector_arg(args[0], who)?;
    let (start, end) = range_args(args, 1, items.len(), who)?;
    Ok(items[start..end].iter().map(Cell::get).collect())
}

/// The bytes of the bytevector `args[0]` in the range `args[1..3]`.
fn bytes(args: &[Value], who: &str) -> Result<Vec<u8>, Error> {
    let b = bytevector_arg(args[0], who)?;
    let (start, end) = range_args(args, 1, b.len(), who)?;
    Ok(b[start..end].iter().map(Cell::get).collect())
}

fn byte(v: Value, who: &str) -> Result<u8, Error> {
    v.as_fixnum().and_then(|n| u8::try_from(n).ok()).ok_or_else(|| expected(who, "a byte (0 to 255)", v))
}

/// The index `at`, if `n` elements copied there fit in `len`.
fn fits(at: Value, n: usize, len: usize, who: &str) -> Result<usize, Error> {
    index_arg(at, who).ok().filter(|at| at + n <= len).ok_or_else(|| err!("{}: the copied elements don't fit at that index", who))
}

