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
    /// Push local `i`: the value in stack slot `fp + i`. Parameters come
    /// first, then `let` variables.
    LocalRef(u16),
    /// Push free variable `i` of the running closure.
    FreeRef(u16),
    /// Push the value of the global whose binding cell is constant `k`.
    /// An error if it is unbound.
    GlobalRef(u32),
    /// Pop a value into the binding cell `k`, and push the variable's name
    /// (the value of a `define`, as in s1).
    GlobalDefine(u32),
    /// Continue at instruction `t` (an index into the template's code).
    Jump(u32),
    /// Pop a value. If it is `#f`, continue at instruction `t`.
    JumpIfFalse(u32),
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
