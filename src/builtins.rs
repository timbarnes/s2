//! Procedures written in Rust (D9, plan/03 §11).
//!
//! A builtin takes its arguments as a slice and returns a value or an error
//! message. It never calls Scheme code, so it never re-enters the VM. The VM
//! checks the argument count against `min`/`max` before calling, so the
//! functions below can index `args` directly.
//!
//! A few procedures need more than the heap: `dis` (and later `eval`) must
//! compile, which needs the top-level environment, and `%frame-depth`
//! looks at the VM. These are *VM primitives* (03 §11): still first-class
//! procedure values, but called with the VM's context.
//!
//! ponytail: P1's arithmetic takes fixnums only (results may be bignums).
//! P6 replaces it with s1's numeric tower. The list and equivalence
//! procedures here are the few that P5's derived syntax (`case` needs
//! `memv`, quasiquote `append` and `list->vector`) and its tests need; P6
//! ports the rest from s1.

use std::io::Write;

use num_bigint::BigInt;

use crate::compile;
use crate::globals::Env;
use crate::heap::{Heap, Obj, eqv, equal, list_to_vec};
use crate::printer::{display_value, print_value};
use crate::value::Value;
use crate::vm::Vm;

pub type BuiltinFn = fn(&mut Heap, &[Value]) -> Result<Value, String>;

/// A VM primitive's code: it also gets the VM and the environment the
/// running code belongs to.
// ponytail: returns a value, which is all `dis` needs. P7 changes this to
// return an `Action` (03 §11), so `eval` can tail-call the code it compiles
// and `%call/cc` can resume a continuation.
pub type VmFn = fn(&mut Vm, &mut Heap, &mut Env, &[Value]) -> Result<Value, String>;

pub enum PrimFn {
    Builtin(BuiltinFn),
    Vm(VmFn),
}

/// A procedure written in Rust: its name, its argument count, and its code.
pub struct Primitive {
    pub name: &'static str,
    pub min: usize,
    /// None for "any number".
    pub max: Option<usize>,
    pub f: PrimFn,
}

const fn prim(name: &'static str, min: usize, max: Option<usize>, f: BuiltinFn) -> Primitive {
    Primitive { name, min, max, f: PrimFn::Builtin(f) }
}

const fn vm_prim(name: &'static str, min: usize, max: Option<usize>, f: VmFn) -> Primitive {
    Primitive { name, min, max, f: PrimFn::Vm(f) }
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
    prim("eq?", 2, Some(2), |_, a| Ok(Value::bool(a[0].eq(a[1])))),
    prim("eqv?", 2, Some(2), |_, a| Ok(Value::bool(eqv(a[0], a[1])))),
    prim("equal?", 2, Some(2), |_, a| Ok(Value::bool(equal(a[0], a[1])))),
    prim("not", 1, Some(1), |_, a| Ok(Value::bool(!a[0].is_true()))),
    prim("null?", 1, Some(1), |_, a| Ok(Value::bool(a[0].is_nil()))),
    prim("pair?", 1, Some(1), |_, a| Ok(Value::bool(pair(a[0], "").is_ok()))),
    prim("list", 0, None, |h, a| Ok(h.list(a.to_vec()))),
    prim("vector", 0, None, |h, a| Ok(h.vector(a.to_vec()))),
    prim("memv", 2, Some(2), memv),
    prim("append", 0, None, append),
    prim("list->vector", 1, Some(1), |h, a| Ok(h.vector(proper_list(a[0], "list->vector")?))),
    prim("display", 1, Some(1), display),
    prim("newline", 0, Some(0), newline),
    vm_prim("dis", 1, Some(1), dis),
    // Like s1's %kont-depth: for tests that tail calls don't grow the stack.
    vm_prim("%frame-depth", 0, Some(0), |vm, _, _, _| Ok(Value::fixnum(vm.frame_depth() as i64).unwrap())),
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
    // Every argument is type-checked, even after the result is known.
    let mut result = true;
    for w in args.windows(2) {
        result &= holds(fixnum(w[0], who)?, fixnum(w[1], who)?);
    }
    Ok(Value::bool(result))
}

fn pair(v: Value, who: &str) -> Result<(Value, Value), String> {
    match v.as_obj().map(|o| &o.kind) {
        Some(Obj::Pair { car, cdr }) => Ok((car.get(), cdr.get())),
        _ => Err(format!("{}: not a pair: {}", who, print_value(v))),
    }
}

fn proper_list(v: Value, who: &str) -> Result<Vec<Value>, String> {
    list_to_vec(v).ok_or_else(|| format!("{}: not a proper list: {}", who, print_value(v)))
}

/// The first tail of the list whose car is `eqv?` to the item, or `#f`.
fn memv(_: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let mut list = args[1];
    while let Ok((car, cdr)) = pair(list, "memv") {
        if eqv(car, args[0]) {
            return Ok(list);
        }
        list = cdr;
    }
    Ok(Value::FALSE)
}

/// The lists appended; the last argument is shared, not copied.
fn append(heap: &mut Heap, args: &[Value]) -> Result<Value, String> {
    let Some((&last, init)) = args.split_last() else { return Ok(Value::NIL) };
    let mut items = Vec::new();
    for &list in init {
        items.extend(proper_list(list, "append")?);
    }
    Ok(items.into_iter().rev().fold(last, |tail, v| heap.cons(v, tail)))
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

/// `(dis datum)`: compile `datum` as a top-level form for the current
/// environment and print its disassembly, without running it.
fn dis(_: &mut Vm, heap: &mut Heap, env: &mut Env, args: &[Value]) -> Result<Value, String> {
    let template = compile::compile(heap, env, args[0])?;
    print!("{}", compile::disasm::disassemble(template));
    Ok(Value::UNSPECIFIED)
}
