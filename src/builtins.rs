//! Procedures written in Rust (D9, plan/03 §11).
//!
//! A builtin takes its arguments as a slice and returns a value or an error
//! message. It never calls Scheme code, so it never re-enters the VM. The VM
//! checks the argument count against `min`/`max` before calling, so the
//! functions below can index `args` directly.
//!
//! ponytail: P1's arithmetic takes fixnums only (results may be bignums).
//! P6 replaces it with s1's numeric tower.

use std::io::Write;

use num_bigint::BigInt;

use crate::globals::Env;
use crate::heap::{Heap, Obj};
use crate::printer::{display_value, print_value};
use crate::value::Value;

pub type BuiltinFn = fn(&mut Heap, &[Value]) -> Result<Value, String>;

/// A builtin procedure: its name, its argument count, and its code.
pub struct Primitive {
    pub name: &'static str,
    pub min: usize,
    /// None for "any number".
    pub max: Option<usize>,
    pub f: BuiltinFn,
}

const fn prim(name: &'static str, min: usize, max: Option<usize>, f: BuiltinFn) -> Primitive {
    Primitive { name, min, max, f }
}

static PRIMITIVES: &[Primitive] = &[
    prim("+", 0, None, add),
    prim("-", 1, None, sub),
    prim("*", 0, None, mul),
    prim("<", 2, None, |_, a| compare(a, "<", |x, y| x < y)),
    prim("=", 2, None, |_, a| compare(a, "=", |x, y| x == y)),
    prim(">", 2, None, |_, a| compare(a, ">", |x, y| x > y)),
    prim("car", 1, Some(1), |_, a| pair(a[0], "car").map(|(car, _)| car)),
    prim("cdr", 1, Some(1), |_, a| pair(a[0], "cdr").map(|(_, cdr)| cdr)),
    prim("cons", 2, Some(2), |h, a| Ok(h.cons(a[0], a[1]))),
    prim("display", 1, Some(1), display),
    prim("newline", 0, Some(0), newline),
];

/// Define every builtin in `env`.
pub fn install(heap: &mut Heap, env: &mut Env) {
    for p in PRIMITIVES {
        let v = heap.primitive(p);
        env.define(heap, p.name, v);
    }
}

fn fixnum(v: Value, who: &str) -> Result<i64, String> {
    v.as_fixnum().ok_or_else(|| format!("{}: not a fixnum: {}", who, print_value(v)))
}

/// An exact integer result, a bignum if it doesn't fit a fixnum.
fn integer(heap: &mut Heap, n: i128) -> Value {
    match i64::try_from(n).ok().and_then(Value::fixnum) {
        Some(v) => v,
        None => heap.integer(BigInt::from(n)),
    }
}

// Sums of 61-bit fixnums can't overflow an i128 for any realistic number of
// arguments.
fn add(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let mut sum: i128 = 0;
    for &a in args {
        sum += fixnum(a, "+")? as i128;
    }
    Ok(integer(heap, sum))
}

fn sub(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let first = fixnum(args[0], "-")? as i128;
    if args.len() == 1 {
        return Ok(integer(heap, -first));
    }
    let mut acc = first;
    for &a in &args[1..] {
        acc -= fixnum(a, "-")? as i128;
    }
    Ok(integer(heap, acc))
}

fn mul(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let mut product = BigInt::from(1);
    for &a in args {
        product *= fixnum(a, "*")?;
    }
    Ok(heap.integer(product))
}

fn compare(args: &[Value], who: &str, holds: fn(i64, i64) -> bool) -> Result<Value, String> {
    let nums = args.iter().map(|&a| fixnum(a, who)).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::bool(nums.windows(2).all(|w| holds(w[0], w[1]))))
}

fn pair(v: Value, who: &str) -> Result<(Value, Value), String> {
    match v.as_obj().map(|o| &o.kind) {
        Some(Obj::Pair { car, cdr }) => Ok((car.get(), cdr.get())),
        _ => Err(format!("{}: not a pair: {}", who, print_value(v))),
    }
}

fn display(_: &mut Heap, args: &[Value]) -> Result<Value, String> {
    print!("{}", display_value(args[0]));
    Ok(Value::UNSPECIFIED)
}

fn newline(_: &mut Heap, _: &[Value]) -> Result<Value, String> {
    println!();
    std::io::stdout().flush().ok();
    Ok(Value::UNSPECIFIED)
}
