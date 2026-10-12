//! Numbers: R7RS §6.2. Forked from s1's `builtin/number.rs`.
//!
//! Four representations: fixnums and flonums in the `Value` itself, and
//! bignums and exact rationals on the heap. Exact arithmetic stays exact
//! (`(/ 1 2)` is `1/2`), whole results come back as integers, and every
//! integer that fits is a fixnum (`Heap::integer`). Any inexact operand
//! makes the result inexact.
//!
//! The common operations (`+ - * < = >` and the integer divisions) have a
//! fast path for fixnum arguments that allocates nothing. Everything else
//! goes through `Num`, s1's working representation.
//!

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::{Error, Primitive, err, expected, kind, prim};
use crate::heap::{Heap, Obj};
use crate::printer::{format_float, print_value};
use crate::reader::number_syntax::{Number, NumberSyntax, parse_number};
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("+", 0, None, add),
    prim("-", 1, None, sub),
    prim("*", 0, None, mul),
    prim("/", 1, None, div),
    prim("=", 2, None, num_eq),
    prim("<", 2, None, lt),
    prim(">", 2, None, gt),
    prim("<=", 2, None, le),
    prim(">=", 2, None, ge),
    prim("number?", 1, Some(1), |_, a| Ok(Value::bool(is_number(a[0])))),
    prim("complex?", 1, Some(1), |_, a| Ok(Value::bool(is_number(a[0])))),
    prim("real?", 1, Some(1), |_, a| Ok(Value::bool(is_number(a[0])))),
    prim("rational?", 1, Some(1), |_, a| Ok(Value::bool(a[0].as_flonum().map_or(is_number(a[0]), f64::is_finite)))),
    prim("integer?", 1, Some(1), |_, a| Ok(Value::bool(is_integer(a[0])))),
    prim("exact-integer?", 1, Some(1), |_, a| Ok(Value::bool(is_exact_integer(a[0])))),
    prim("exact?", 1, Some(1), |_, a| Ok(Value::bool(Num::of(a[0], "exact?")?.is_exact()))),
    prim("inexact?", 1, Some(1), |_, a| Ok(Value::bool(!Num::of(a[0], "inexact?")?.is_exact()))),
    prim("nan?", 1, Some(1), |_, a| float_pred(a[0], "nan?", f64::is_nan, false)),
    prim("infinite?", 1, Some(1), |_, a| float_pred(a[0], "infinite?", f64::is_infinite, false)),
    prim("finite?", 1, Some(1), |_, a| float_pred(a[0], "finite?", f64::is_finite, true)),
    prim("zero?", 1, Some(1), |_, a| Ok(Value::bool(Num::of(a[0], "zero?")?.sign() == Some(Ordering::Equal)))),
    prim("positive?", 1, Some(1), |_, a| Ok(Value::bool(Num::of(a[0], "positive?")?.sign() == Some(Ordering::Greater)))),
    prim("negative?", 1, Some(1), |_, a| Ok(Value::bool(Num::of(a[0], "negative?")?.sign() == Some(Ordering::Less)))),
    prim("odd?", 1, Some(1), |_, a| Ok(Value::bool(integer(a[0], "odd?")?.is_odd()))),
    prim("even?", 1, Some(1), |_, a| Ok(Value::bool(integer(a[0], "even?")?.is_even()))),
    prim("max", 1, None, |h, a| extreme(h, a, "max", Ordering::Greater)),
    prim("min", 1, None, |h, a| extreme(h, a, "min", Ordering::Less)),
    prim("abs", 1, Some(1), abs),
    prim("quotient", 2, Some(2), |h, a| int_div(h, a, "quotient", |x, y| x / y, |x, y| x.checked_div(y))),
    prim("remainder", 2, Some(2), |h, a| int_div(h, a, "remainder", |x, y| x % y, |x, y| x.checked_rem(y))),
    prim("modulo", 2, Some(2), |h, a| int_div(h, a, "modulo", |x, y| Integer::mod_floor(&x, &y), |x, y| (y != 0).then(|| Integer::mod_floor(&x, &y)))),
    prim("truncate-quotient", 2, Some(2), |h, a| int_div(h, a, "truncate-quotient", |x, y| x / y, |x, y| x.checked_div(y))),
    prim("truncate-remainder", 2, Some(2), |h, a| int_div(h, a, "truncate-remainder", |x, y| x % y, |x, y| x.checked_rem(y))),
    prim("floor-quotient", 2, Some(2), |h, a| int_div(h, a, "floor-quotient", |x, y| Integer::div_floor(&x, &y), |x, y| (y != 0).then(|| Integer::div_floor(&x, &y)))),
    prim("floor-remainder", 2, Some(2), |h, a| int_div(h, a, "floor-remainder", |x, y| Integer::mod_floor(&x, &y), |x, y| (y != 0).then(|| Integer::mod_floor(&x, &y)))),
    prim("floor/", 2, Some(2), |h, a| int_div2(h, a, "floor/", |x, y| x.div_mod_floor(&y))),
    prim("truncate/", 2, Some(2), |h, a| int_div2(h, a, "truncate/", |x, y| x.div_rem(&y))),
    prim("exact-integer-sqrt", 1, Some(1), |h, a| {
        let k = match Num::of(a[0], "exact-integer-sqrt")? {
            Num::Int(k) if !k.is_negative() => k,
            _ => return Err(expected("exact-integer-sqrt", "a non-negative exact integer", a[0])),
        };
        let s = k.sqrt();
        let r = &k - &s * &s;
        let vals = vec![h.integer(s), h.integer(r)];
        Ok(h.values(vals))
    }),
    prim("gcd", 0, None, |h, a| gcd_lcm(h, a, "gcd", false)),
    prim("lcm", 0, None, |h, a| gcd_lcm(h, a, "lcm", true)),
    prim("numerator", 1, Some(1), |h, a| ratio_part(h, a[0], "numerator", true)),
    prim("denominator", 1, Some(1), |h, a| ratio_part(h, a[0], "denominator", false)),
    prim("floor", 1, Some(1), |h, a| rounding(h, a[0], "floor", |r| r.floor().to_integer(), f64::floor)),
    prim("ceiling", 1, Some(1), |h, a| rounding(h, a[0], "ceiling", |r| r.ceil().to_integer(), f64::ceil)),
    prim("truncate", 1, Some(1), |h, a| rounding(h, a[0], "truncate", |r| r.trunc().to_integer(), f64::trunc)),
    prim("round", 1, Some(1), |h, a| rounding(h, a[0], "round", round_half_even, f64::round_ties_even)),
    prim("rationalize", 2, Some(2), rationalize),
    prim("exp", 1, Some(1), |_, a| float_fn(a[0], "exp", f64::exp)),
    prim("sin", 1, Some(1), |_, a| float_fn(a[0], "sin", f64::sin)),
    prim("cos", 1, Some(1), |_, a| float_fn(a[0], "cos", f64::cos)),
    prim("tan", 1, Some(1), |_, a| float_fn(a[0], "tan", f64::tan)),
    prim("asin", 1, Some(1), |_, a| float_fn(a[0], "asin", f64::asin)),
    prim("acos", 1, Some(1), |_, a| float_fn(a[0], "acos", f64::acos)),
    prim("log", 1, Some(2), log),
    prim("atan", 1, Some(2), atan),
    prim("square", 1, Some(1), |h, a| mul(h, &[a[0], a[0]])),
    prim("sqrt", 1, Some(1), sqrt),
    prim("expt", 2, Some(2), expt),
    prim("exact", 1, Some(1), exact),
    prim("inexact", 1, Some(1), inexact),
    prim("exact->inexact", 1, Some(1), inexact),
    prim("inexact->exact", 1, Some(1), exact),
    prim("number->string", 1, Some(2), number_to_string),
    prim("string->number", 1, Some(2), string_to_number),
];

// ---------------------------------------------------------------------------
// The working representation
// ---------------------------------------------------------------------------

/// A number taken out of a `Value` for computation.
#[derive(Clone, Debug, PartialEq)]
enum Num {
    Int(BigInt),
    /// Always a non-integer: see `Num::from_ratio`.
    Rat(BigRational),
    Float(f64),
}

pub fn is_number(v: Value) -> bool {
    v.as_fixnum().is_some() || v.as_flonum().is_some() || matches!(kind(v), Some(Obj::BigInt(_) | Obj::Rational(_)))
}

fn is_exact_integer(v: Value) -> bool {
    v.as_fixnum().is_some() || matches!(kind(v), Some(Obj::BigInt(_)))
}

fn is_integer(v: Value) -> bool {
    is_exact_integer(v) || v.as_flonum().is_some_and(|f| f.is_finite() && f.fract() == 0.0)
}

impl Num {
    /// The number `v` holds, or an error naming `who`.
    fn of(v: Value, who: &str) -> Result<Num, Error> {
        if let Some(n) = v.as_fixnum() {
            return Ok(Num::Int(BigInt::from(n)));
        }
        if let Some(f) = v.as_flonum() {
            return Ok(Num::Float(f));
        }
        match kind(v) {
            Some(Obj::BigInt(i)) => Ok(Num::Int(i.clone())),
            Some(Obj::Rational(r)) => Ok(Num::Rat(r.clone())),
            _ => Err(err!("{}: expected a number, got {}", who, print_value(v))),
        }
    }

    /// The exact number `r`, as an `Int` when it is whole.
    fn from_ratio(r: BigRational) -> Num {
        if r.is_integer() { Num::Int(r.to_integer()) } else { Num::Rat(r) }
    }

    fn alloc(self, heap: &mut Heap) -> Value {
        match self {
            Num::Int(i) => heap.integer(i),
            Num::Rat(r) => heap.rational(r),
            Num::Float(f) => Value::flonum(f),
        }
    }

    fn is_exact(&self) -> bool {
        !matches!(self, Num::Float(_))
    }

    fn to_f64(&self) -> f64 {
        match self {
            Num::Int(i) => int_to_f64(i),
            Num::Rat(r) => r.to_f64().unwrap_or(f64::NAN),
            Num::Float(f) => *f,
        }
    }

    /// The exact value of an exact number.
    fn to_ratio(&self) -> BigRational {
        match self {
            Num::Int(i) => BigRational::from_integer(i.clone()),
            Num::Rat(r) => r.clone(),
            Num::Float(f) => BigRational::from_float(*f).unwrap_or_else(BigRational::zero),
        }
    }

    fn into_exact(self, who: &str) -> Result<Num, Error> {
        match self {
            Num::Float(f) => float_to_exact(f, who),
            n => Ok(n),
        }
    }

    /// The sign, or None for a NaN.
    fn sign(&self) -> Option<Ordering> {
        match self {
            Num::Int(i) => Some(i.sign().cmp(&num_bigint::Sign::NoSign)),
            Num::Rat(r) => Some(r.numer().sign().cmp(&num_bigint::Sign::NoSign)),
            Num::Float(f) => f.partial_cmp(&0.0),
        }
    }

    /// The integer this number is, exact or inexact, if it is one.
    fn as_integer(&self) -> Option<BigInt> {
        match self {
            Num::Int(i) => Some(i.clone()),
            Num::Float(f) if f.is_finite() && f.fract() == 0.0 => BigRational::from_float(*f).map(|r| r.to_integer()),
            _ => None,
        }
    }
}

fn int_to_f64(i: &BigInt) -> f64 {
    i.to_f64().unwrap_or(if i.is_negative() { f64::NEG_INFINITY } else { f64::INFINITY })
}

/// The exact value of a flonum (`(exact 0.5)` is `1/2`).
fn float_to_exact(f: f64, who: &str) -> Result<Num, Error> {
    BigRational::from_float(f).map(Num::from_ratio).ok_or_else(|| err!("{}: {} has no exact representation", who, format_float(f)))
}

/// The integer `v`, exact or inexact.
fn integer(v: Value, who: &str) -> Result<BigInt, Error> {
    if let Some(n) = v.as_fixnum() {
        return Ok(BigInt::from(n));
    }
    Num::of(v, who)?.as_integer().ok_or_else(|| err!("{}: expected an integer, got {}", who, print_value(v)))
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Op {
    Add,
    Sub,
    Mul,
}

/// `acc op x`, staying exact when both are exact.
fn combine(acc: Num, x: Num, op: Op) -> Num {
    match (acc, x) {
        (Num::Int(a), Num::Int(b)) => Num::Int(match op {
            Op::Add => a + b,
            Op::Sub => a - b,
            Op::Mul => a * b,
        }),
        (a @ Num::Float(_), b) | (a, b @ Num::Float(_)) => {
            let (a, b) = (a.to_f64(), b.to_f64());
            Num::Float(match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
            })
        }
        (a, b) => {
            let (a, b) = (a.to_ratio(), b.to_ratio());
            Num::from_ratio(match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
            })
        }
    }
}

/// The fold of `op` over `args` from `init`, for any numbers.
fn fold(heap: &mut Heap, init: Num, args: &[Value], op: Op, who: &str) -> Result<Value, Error> {
    let mut acc = init;
    for &a in args {
        acc = combine(acc, Num::of(a, who)?, op);
    }
    Ok(acc.alloc(heap))
}

/// The fixnums in `args`, if every argument is one.
fn all_fixnums(args: &[Value]) -> Option<impl Iterator<Item = i128> + '_> {
    args.iter().all(|a| a.as_fixnum().is_some()).then(|| args.iter().map(|a| a.as_fixnum().unwrap() as i128))
}

fn int128(heap: &mut Heap, n: i128) -> Value {
    match i64::try_from(n).ok().and_then(Value::fixnum) {
        Some(v) => v,
        None => heap.integer(BigInt::from(n)),
    }
}

// Sums of 61-bit fixnums can't overflow an i128 for any realistic number of
// arguments.
pub fn add(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    if let Some(ns) = all_fixnums(args) {
        return Ok(int128(heap, ns.sum()));
    }
    fold(heap, Num::Int(BigInt::zero()), args, Op::Add, "+")
}

pub fn sub(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    if let Some(mut ns) = all_fixnums(args) {
        let first = ns.next().unwrap();
        return Ok(int128(heap, if args.len() == 1 { -first } else { ns.fold(first, |a, b| a - b) }));
    }
    if args.len() == 1 {
        // Negation, not 0 - x, which would lose the sign of a zero:
        // (- 0.0) is -0.0.
        return Ok(match Num::of(args[0], "-")? {
            Num::Int(i) => Num::Int(-i),
            Num::Rat(r) => Num::Rat(-r),
            Num::Float(f) => Num::Float(-f),
        }
        .alloc(heap));
    }
    fold(heap, Num::of(args[0], "-")?, &args[1..], Op::Sub, "-")
}

pub fn mul(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    if let Some(mut ns) = all_fixnums(args)
        && let Some(p) = ns.try_fold(1i128, |a, b| a.checked_mul(b))
    {
        return Ok(int128(heap, p));
    }
    fold(heap, Num::Int(BigInt::one()), args, Op::Mul, "*")
}

/// `a / b`. Exact division by exact zero is an error; inexact division
/// follows IEEE (`(/ 1.0 0)` is `+inf.0`).
fn divide(a: Num, b: Num) -> Result<Num, Error> {
    if a.is_exact() && b.is_exact() {
        if b.sign() == Some(Ordering::Equal) {
            return Err("/: division by zero".into());
        }
        Ok(Num::from_ratio(a.to_ratio() / b.to_ratio()))
    } else {
        Ok(Num::Float(a.to_f64() / b.to_f64()))
    }
}

pub fn div(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let first = Num::of(args[0], "/")?;
    let mut acc = if args.len() == 1 { divide(Num::Int(BigInt::one()), first)? } else { first };
    for &a in &args[1..] {
        acc = divide(acc, Num::of(a, "/")?)?;
    }
    Ok(acc.alloc(heap))
}

fn abs(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    Ok(match Num::of(args[0], "abs")? {
        Num::Int(i) => Num::Int(i.abs()),
        Num::Rat(r) => Num::Rat(r.abs()),
        Num::Float(f) => Num::Float(f.abs()),
    }
    .alloc(heap))
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// Compare two numbers exactly. A finite flonum is compared by its exact
/// value, so comparison is transitive even where converting the exact side
/// to `f64` would round. None when a NaN is involved.
fn num_cmp(a: &Num, b: &Num) -> Option<Ordering> {
    match (a, b) {
        (Num::Int(x), Num::Int(y)) => Some(x.cmp(y)),
        (Num::Float(x), Num::Float(y)) => x.partial_cmp(y),
        (Num::Int(x), Num::Float(y)) if x.bits() <= 53 => int_to_f64(x).partial_cmp(y),
        (Num::Float(x), Num::Int(y)) if y.bits() <= 53 => x.partial_cmp(&int_to_f64(y)),
        (Num::Float(x), _) => exact_vs_float(b, *x).map(Ordering::reverse),
        (_, Num::Float(y)) => exact_vs_float(a, *y),
        (x, y) => Some(x.to_ratio().cmp(&y.to_ratio())),
    }
}

/// Order an exact number against a flonum.
fn exact_vs_float(exact: &Num, f: f64) -> Option<Ordering> {
    if f.is_nan() {
        return None;
    }
    if f.is_infinite() {
        return Some(if f > 0.0 { Ordering::Less } else { Ordering::Greater });
    }
    Some(exact.to_ratio().cmp(&BigRational::from_float(f)?))
}

pub fn num_eq(_: &mut Heap, a: &[Value]) -> Result<Value, Error> {
    compare(a, "=", Ordering::is_eq)
}

pub fn lt(_: &mut Heap, a: &[Value]) -> Result<Value, Error> {
    compare(a, "<", Ordering::is_lt)
}

pub fn gt(_: &mut Heap, a: &[Value]) -> Result<Value, Error> {
    compare(a, ">", Ordering::is_gt)
}

pub fn le(_: &mut Heap, a: &[Value]) -> Result<Value, Error> {
    compare(a, "<=", Ordering::is_le)
}

pub fn ge(_: &mut Heap, a: &[Value]) -> Result<Value, Error> {
    compare(a, ">=", Ordering::is_ge)
}

fn compare(args: &[Value], who: &str, ok: fn(Ordering) -> bool) -> Result<Value, Error> {
    if args.iter().all(|a| a.as_fixnum().is_some()) {
        return Ok(Value::bool(args.windows(2).all(|w| ok(w[0].as_fixnum().cmp(&w[1].as_fixnum())))));
    }
    // Every argument is type-checked, even after the result is known.
    let ns = args.iter().map(|&a| Num::of(a, who)).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::bool(ns.windows(2).all(|w| num_cmp(&w[0], &w[1]).is_some_and(ok))))
}

/// `max` / `min`: the extreme argument, made inexact if any argument is.
fn extreme(heap: &mut Heap, args: &[Value], who: &str, want: Ordering) -> Result<Value, Error> {
    let ns = args.iter().map(|&a| Num::of(a, who)).collect::<Result<Vec<_>, _>>()?;
    let inexact = ns.iter().any(|n| !n.is_exact());
    if let Some(nan) = ns.iter().find(|n| matches!(n, Num::Float(f) if f.is_nan())) {
        return Ok(nan.clone().alloc(heap));
    }
    let mut best = &ns[0];
    for n in &ns[1..] {
        if num_cmp(n, best) == Some(want) {
            best = n;
        }
    }
    let best = if inexact { Num::Float(best.to_f64()) } else { best.clone() };
    Ok(best.alloc(heap))
}

fn float_pred(v: Value, who: &str, test: fn(f64) -> bool, exact: bool) -> Result<Value, Error> {
    Ok(Value::bool(match Num::of(v, who)? {
        Num::Float(f) => test(f),
        _ => exact,
    }))
}

// ---------------------------------------------------------------------------
// Integer division
// ---------------------------------------------------------------------------

/// An integer division: `fast` on two fixnums (None for division by zero),
/// `big` on any two integers. An inexact (integral flonum) operand gives an
/// inexact result.
fn int_div(heap: &mut Heap, args: &[Value], who: &str, big: fn(BigInt, BigInt) -> BigInt, fast: fn(i64, i64) -> Option<i64>) -> Result<Value, Error> {
    if let (Some(x), Some(y)) = (args[0].as_fixnum(), args[1].as_fixnum()) {
        // The only fixnum quotient that isn't a fixnum is FIXNUM_MIN / -1,
        // which `Value::fixnum` turns away; it then takes the slow path.
        match fast(x, y).map(Value::fixnum) {
            Some(Some(v)) => return Ok(v),
            None => return Err(err!("{}: division by zero", who)),
            Some(None) => {}
        }
    }
    let (a, b) = (Num::of(args[0], who)?, Num::of(args[1], who)?);
    let inexact = !a.is_exact() || !b.is_exact();
    let to_int = |n: &Num| n.as_integer().ok_or_else(|| err!("{}: expected integers", who));
    let (x, y) = (to_int(&a)?, to_int(&b)?);
    if y.is_zero() {
        return Err(err!("{}: division by zero", who));
    }
    Ok(int_result(heap, big(x, y), inexact))
}

/// `floor/` and `truncate/`: the quotient and remainder, as two values.
fn int_div2(heap: &mut Heap, args: &[Value], who: &str, f: fn(BigInt, BigInt) -> (BigInt, BigInt)) -> Result<Value, Error> {
    let (a, b) = (Num::of(args[0], who)?, Num::of(args[1], who)?);
    let inexact = !a.is_exact() || !b.is_exact();
    let to_int = |n: &Num| n.as_integer().ok_or_else(|| err!("{}: expected integers", who));
    let (x, y) = (to_int(&a)?, to_int(&b)?);
    if y.is_zero() {
        return Err(err!("{}: division by zero", who));
    }
    let (q, r) = f(x, y);
    let vals = vec![int_result(heap, q, inexact), int_result(heap, r, inexact)];
    Ok(heap.values(vals))
}

fn int_result(heap: &mut Heap, n: BigInt, inexact: bool) -> Value {
    if inexact { Value::flonum(int_to_f64(&n)) } else { heap.integer(n) }
}

fn gcd_lcm(heap: &mut Heap, args: &[Value], who: &str, lcm: bool) -> Result<Value, Error> {
    let mut acc = if lcm { BigInt::one() } else { BigInt::zero() };
    let mut inexact = false;
    for &a in args {
        let n = Num::of(a, who)?;
        inexact |= !n.is_exact();
        let i = n.as_integer().ok_or_else(|| err!("{}: expected integers", who))?;
        acc = if lcm { acc.lcm(&i) } else { acc.gcd(&i) };
    }
    Ok(int_result(heap, acc.abs(), inexact))
}

// ---------------------------------------------------------------------------
// Rationals and rounding
// ---------------------------------------------------------------------------

/// `numerator` / `denominator`. For a flonum these are of its exact value,
/// returned inexact: `(denominator 0.5)` is `2.0`.
fn ratio_part(heap: &mut Heap, v: Value, who: &str, numer: bool) -> Result<Value, Error> {
    let n = Num::of(v, who)?;
    let inexact = !n.is_exact();
    let r = n.into_exact(who)?.to_ratio();
    let part = if numer { r.numer().clone() } else { r.denom().clone() };
    Ok(int_result(heap, part, inexact))
}

/// Round an exact rational to an integer, halves to even.
fn round_half_even(r: &BigRational) -> BigInt {
    let floor = r.floor();
    let diff = r - &floor;
    let half = BigRational::new(BigInt::one(), BigInt::from(2));
    let floor = floor.to_integer();
    match diff.cmp(&half) {
        Ordering::Less => floor,
        Ordering::Greater => floor + 1,
        Ordering::Equal if floor.is_even() => floor,
        Ordering::Equal => floor + 1,
    }
}

/// `floor`, `ceiling`, `truncate` and `round`: integers are returned as
/// they are, rationals become exact integers, flonums stay flonums.
fn rounding(heap: &mut Heap, v: Value, who: &str, exact: fn(&BigRational) -> BigInt, inexact: fn(f64) -> f64) -> Result<Value, Error> {
    Ok(match Num::of(v, who)? {
        Num::Int(_) => v,
        Num::Rat(r) => heap.integer(exact(&r)),
        Num::Float(f) => Value::flonum(inexact(f)),
    })
}

/// The simplest rational in the closed interval [lo, hi] (lo <= hi).
fn simplest_between(lo: &BigRational, hi: &BigRational) -> BigRational {
    if !lo.is_positive() && !hi.is_negative() {
        BigRational::zero()
    } else if hi.is_negative() {
        -simplest_between(&-hi, &-lo)
    } else {
        simplest_positive(lo, hi)
    }
}

/// `simplest_between` for 0 < lo <= hi.
fn simplest_positive(lo: &BigRational, hi: &BigRational) -> BigRational {
    let fl = lo.floor();
    if &fl == lo {
        return fl;
    }
    let next = &fl + BigRational::one();
    if &next <= hi {
        return next;
    }
    // lo and hi lie strictly inside (fl, fl + 1): recurse on the reciprocals
    // of their fractional parts.
    let inner = simplest_positive(&(hi - &fl).recip(), &(lo - &fl).recip());
    fl + inner.recip()
}

fn rationalize(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let x = Num::of(args[0], "rationalize")?;
    let y = Num::of(args[1], "rationalize")?;
    let inexact = !x.is_exact() || !y.is_exact();
    let (xf, yf) = (x.to_f64(), y.to_f64());
    if inexact && (!xf.is_finite() || !yf.is_finite()) {
        let result = if xf.is_nan() || yf.is_nan() || (xf.is_infinite() && yf.is_infinite()) {
            f64::NAN
        } else if yf.is_infinite() {
            0.0
        } else {
            xf
        };
        return Ok(Value::flonum(result));
    }
    let x = x.into_exact("rationalize")?.to_ratio();
    let y = y.into_exact("rationalize")?.to_ratio().abs();
    let r = Num::from_ratio(simplest_between(&(&x - &y), &(&x + &y)));
    Ok(if inexact { Num::Float(r.to_f64()) } else { r }.alloc(heap))
}

// ---------------------------------------------------------------------------
// Transcendental functions, roots and powers
// ---------------------------------------------------------------------------

fn float_fn(v: Value, who: &str, f: fn(f64) -> f64) -> Result<Value, Error> {
    Ok(Value::flonum(f(Num::of(v, who)?.to_f64())))
}

fn log(_: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let z = Num::of(args[0], "log")?.to_f64();
    Ok(Value::flonum(match args.get(1) {
        Some(&base) => z.ln() / Num::of(base, "log")?.to_f64().ln(),
        None => z.ln(),
    }))
}

fn atan(_: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let y = Num::of(args[0], "atan")?.to_f64();
    Ok(Value::flonum(match args.get(1) {
        Some(&x) => y.atan2(Num::of(x, "atan")?.to_f64()),
        None => y.atan(),
    }))
}

/// The exact square root of a non-negative integer, if it has one.
fn exact_root(n: &BigInt) -> Option<BigInt> {
    let s = n.sqrt();
    (&s * &s == *n).then_some(s)
}

fn sqrt(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let n = Num::of(args[0], "sqrt")?;
    if n.sign() == Some(Ordering::Less) {
        return Err("sqrt: complex results are not supported".into());
    }
    let exact = match &n {
        Num::Int(i) => exact_root(i).map(Num::Int),
        Num::Rat(r) => match (exact_root(r.numer()), exact_root(r.denom())) {
            (Some(a), Some(b)) => Some(Num::from_ratio(BigRational::new(a, b))),
            _ => None,
        },
        Num::Float(_) => None,
    };
    Ok(exact.unwrap_or_else(|| Num::Float(n.to_f64().sqrt())).alloc(heap))
}

/// Exponents above this are refused for exact powers, which would otherwise
/// try to build an astronomically large integer.
const MAX_EXACT_POWER: u32 = 1 << 24;

fn expt(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let base = Num::of(args[0], "expt")?;
    let exp = Num::of(args[1], "expt")?;
    if let Num::Int(e) = &exp {
        if base.is_exact() {
            return Ok(exact_power(base, e)?.alloc(heap));
        }
        if let Some(e) = e.to_i32() {
            return Ok(Value::flonum(base.to_f64().powi(e)));
        }
    }
    let (b, e) = (base.to_f64(), exp.to_f64());
    let result = b.powf(e);
    if result.is_nan() && !b.is_nan() && !e.is_nan() {
        return Err("expt: complex results are not supported".into());
    }
    Ok(Value::flonum(result))
}

fn exact_power(base: Num, e: &BigInt) -> Result<Num, Error> {
    let r = base.to_ratio();
    // 0, 1 and -1 stay small whatever the exponent.
    if r.is_zero() {
        return match e.sign() {
            num_bigint::Sign::Minus => Err("expt: division by zero".into()),
            num_bigint::Sign::NoSign => Ok(Num::Int(BigInt::one())),
            num_bigint::Sign::Plus => Ok(Num::Int(BigInt::zero())),
        };
    }
    if r.abs().is_one() {
        let odd = e.is_odd();
        return Ok(Num::Int(if r.is_negative() && odd { -BigInt::one() } else { BigInt::one() }));
    }
    let mag = e.abs().to_u32().filter(|m| *m <= MAX_EXACT_POWER).ok_or("expt: exponent too large")?;
    let p = num_traits::pow(r, mag as usize);
    Ok(Num::from_ratio(if e.is_negative() { p.recip() } else { p }))
}

fn exact(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    match Num::of(args[0], "exact")? {
        Num::Float(f) => Ok(float_to_exact(f, "exact")?.alloc(heap)),
        _ => Ok(args[0]),
    }
}

fn inexact(_: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    Ok(Value::flonum(Num::of(args[0], "inexact")?.to_f64()))
}

// ---------------------------------------------------------------------------
// Conversion to and from strings
// ---------------------------------------------------------------------------

fn radix_arg(args: &[Value], who: &str) -> Result<u32, Error> {
    match args.get(1) {
        None => Ok(10),
        Some(r) => match r.as_fixnum() {
            Some(r @ (2 | 8 | 10 | 16)) => Ok(r as u32),
            _ => Err(err!("{}: radix must be 2, 8, 10 or 16", who)),
        },
    }
}

fn number_to_string(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let radix = radix_arg(args, "number->string")?;
    let s = match Num::of(args[0], "number->string")? {
        Num::Int(i) => i.to_str_radix(radix),
        Num::Rat(r) => format!("{}/{}", r.numer().to_str_radix(radix), r.denom().to_str_radix(radix)),
        Num::Float(f) if radix == 10 => format_float(f),
        Num::Float(_) => return Err("number->string: inexact numbers support only radix 10".into()),
    };
    Ok(heap.string(&s))
}

fn string_to_number(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let radix = radix_arg(args, "string->number")?;
    let text = super::string_arg(args[0], "string->number")?.borrow().to_string();
    Ok(match parse_number(&text, radix) {
        NumberSyntax::Value(Number::Int(i)) => heap.integer(i),
        NumberSyntax::Value(Number::Rational(r)) => heap.rational(r),
        NumberSyntax::Value(Number::Float(f)) => Value::flonum(f),
        NumberSyntax::NotANumber | NumberSyntax::Error(_) => Value::FALSE,
    })
}
