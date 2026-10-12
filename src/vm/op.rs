//! The instruction set (D5) and templates.
//!
//! An instruction is a Rust enum value ("wordcode"), 8 bytes, dispatched by
//! a `match`. Every expression's code leaves exactly one value on the
//! stack, which is what makes code generation a simple recursive walk. The
//! exceptions are `TailCall` and `Return`, which leave the current frame.

use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    /// Push constant `k` from the template's pool.
    Const(u32),
    /// Push a small integer, with no pool entry.
    Fixnum(i32),
    Nil,
    True,
    False,
    Unspecified,
    /// Push the undefined value: the contents of a `letrec` variable before
    /// its init has run.
    Undefined,
    /// Push local `i`: the value in stack slot `fp + i`. Parameters come
    /// first, then `let` variables.
    LocalRef(u16),
    /// Pop a value into local `i`.
    LocalSet(u16),
    /// Push free variable `i` of the running closure.
    FreeRef(u16),
    /// `PatchFree(c, i, v)`: set free variable `i` of the closure in local
    /// `c` to the value of local `v`. A `letrec` whose inits are all
    /// lambdas makes its closures first, capturing placeholders for the
    /// members not made yet, then fills those in (*Fixing Letrec*).
    PatchFree(u16, u16, u16),
    /// Replace the value in local `i` with a new box holding it. Done once,
    /// where a boxed variable is bound (03 §7).
    MakeBox(u16),
    /// Pop a box and push its contents. An error if the box is still
    /// undefined: a `letrec` variable used before its init has run.
    BoxRef,
    /// Pop a value, then a box, and put the value in the box.
    BoxSet,
    /// Push the value of the global whose binding cell is constant `k`.
    /// An error if it is unbound.
    GlobalRef(u32),
    /// Pop a value into the binding cell `k` (`set!`). An error if the
    /// variable is unbound.
    GlobalSet(u32),
    /// Pop a value into the binding cell `k`, and push the variable's name
    /// (the value of a `define`, as in s1).
    GlobalDefine(u32),
    /// Continue at instruction `t` (an index into the template's code).
    Jump(u32),
    /// Pop a value. If it is `#f`, continue at instruction `t`.
    JumpIfFalse(u32),
    /// A superinstruction (P10): an inlined test followed by `JumpIfFalse`,
    /// the test of nearly every `if` in a loop. Pop the test's arguments; if
    /// the test is false, continue at instruction `t`.
    JumpIfNot(Test, u32),
    /// Discard the top value.
    Pop,
    /// Keep the top value, and discard the `n` values below it. Ends a
    /// `let` that isn't in tail position, removing its variables.
    Slide(u16),
    /// Constant `k` is a template with `nfree` free variables, whose values
    /// are on the stack. Replace them with a new closure.
    MakeClosure(u32),
    /// The stack holds a procedure and `n` arguments above it. Call the
    /// procedure; when it returns, all `n + 1` values have been replaced
    /// with the result.
    Call(u16),
    /// A call in tail position: replace the current frame with the callee's,
    /// so the callee returns directly to this procedure's caller.
    TailCall(u16),
    /// Pop a value and return it to the caller.
    Return,
    /// A tail call from a procedure to itself with `n` arguments: a loop
    /// (P10). Codegen emits it only where the callee is known to be the
    /// running closure (a named `let`'s own name) and the argument count is
    /// right, so it just moves the arguments into the parameter slots and
    /// starts again.
    SelfTailCall(u16),
    // Inlined primitives (plan/03 §11): a call to one of these standard
    // procedures, with this many arguments, compiles to one instruction
    // instead of `GlobalRef` and `Call`. Each replaces its arguments on the
    // stack with its result. The common case (a pair; fixnums or flonums;
    // a vector and an index in range) is done in the dispatch loop;
    // anything else calls the builtin itself, so the results and errors are
    // the builtin's.
    Car,
    Cdr,
    Cons,
    NullP,
    PairP,
    EqP,
    Not,
    Add,
    Sub,
    Mul,
    Div,
    NumEq,
    Lt,
    Gt,
    Le,
    Ge,
    VectorLength,
    VectorRef,
    VectorSet,
    // Superinstructions (after P12), chosen from the instruction counts
    // of the r7rs benchmarks. Each does what the two instructions it
    // replaces would, fast and slow path alike.
    /// `LocalRef(i)` then `Car`: `car` of an unboxed local.
    CarLocal(u16),
    /// `LocalRef(i)` then `Cdr`.
    CdrLocal(u16),
    /// `Fixnum(k)` then `Add`: add a constant.
    AddImm(i32),
    /// `Fixnum(k)` then `Sub`: subtract a constant.
    SubImm(i32),
    /// `Fixnum(k)` then `JumpIfNot(test, t)`: compare a value with a
    /// constant, popping it, and jump to `t` unless the comparison holds.
    JumpIfNotImm(Test, i16, u32),
}

/// The tests `JumpIfNot` can make: the inlined predicates and
/// comparisons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Test {
    NullP,
    PairP,
    EqP,
    NumEq,
    Lt,
    Gt,
    Le,
    Ge,
}

impl Test {
    /// The test an inlined instruction makes, if it is one.
    pub fn of(op: Op) -> Option<Test> {
        Some(match op {
            Op::NullP => Test::NullP,
            Op::PairP => Test::PairP,
            Op::EqP => Test::EqP,
            Op::NumEq => Test::NumEq,
            Op::Lt => Test::Lt,
            Op::Gt => Test::Gt,
            Op::Le => Test::Le,
            Op::Ge => Test::Ge,
            _ => return None,
        })
    }

    pub fn arity(self) -> usize {
        if matches!(self, Test::NullP | Test::PairP) { 1 } else { 2 }
    }
}

impl Op {
    /// The instruction a call to the standard procedure `name` with `argc`
    /// arguments can be inlined as.
    pub fn inline(name: &str, argc: usize) -> Option<Op> {
        Some(match (name, argc) {
            ("car", 1) => Op::Car,
            ("cdr", 1) => Op::Cdr,
            ("cons", 2) => Op::Cons,
            ("null?", 1) => Op::NullP,
            ("pair?", 1) => Op::PairP,
            ("eq?", 2) => Op::EqP,
            ("not", 1) => Op::Not,
            ("+", 2) => Op::Add,
            ("-", 2) => Op::Sub,
            ("*", 2) => Op::Mul,
            ("/", 2) => Op::Div,
            ("=", 2) => Op::NumEq,
            ("<", 2) => Op::Lt,
            (">", 2) => Op::Gt,
            ("<=", 2) => Op::Le,
            (">=", 2) => Op::Ge,
            ("vector-length", 1) => Op::VectorLength,
            ("vector-ref", 2) => Op::VectorRef,
            ("vector-set!", 3) => Op::VectorSet,
            _ => return None,
        })
    }
}

const _: () = assert!(std::mem::size_of::<Op>() == 8);

/// Compiled code for one procedure: a `lambda`, or a top-level form (a
/// procedure of no arguments).
pub struct Template {
    /// The procedure's name; empty for an anonymous `lambda`.
    pub name: Box<str>,
    /// The number of required parameters.
    pub required: u16,
    /// Whether extra arguments are collected into a rest list, which becomes
    /// local `required`.
    pub rest: bool,
    /// The number of free variables a closure of this template holds.
    pub nfree: u16,
    pub code: Vec<Op>,
    /// Constants: literals, binding cells and the templates of inner
    /// lambdas, referred to by index.
    pub consts: Vec<Value>,
    /// Names for the disassembler: of each local slot (a slot reused by
    /// sibling `let`s has several, joined by `/`), and of each free
    /// variable.
    pub local_names: Vec<String>,
    pub free_names: Vec<String>,
}
