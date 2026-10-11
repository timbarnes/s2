//! The instruction set (D5) and templates.
//!
//! An instruction is a Rust enum value ("wordcode"), 8 bytes, dispatched by
//! a `match`. Every expression's code leaves exactly one value on the
//! stack, which is what makes code generation a simple recursive walk.

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
    /// The stack holds a procedure and `n` arguments above it. Call the
    /// procedure, and replace all `n + 1` values with the result.
    Call(u16),
    /// Pop a value and return it from the template.
    Return,
}

const _: () = assert!(std::mem::size_of::<Op>() == 8);

/// A unit of compiled code: P1 compiles each top-level form into one
/// template, which the VM runs as a procedure of no arguments.
pub struct Template {
    pub name: Box<str>,
    pub code: Vec<Op>,
    /// Constants: literals and binding cells, referred to by index.
    pub consts: Vec<Value>,
}
