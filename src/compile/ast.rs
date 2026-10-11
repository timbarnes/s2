//! The core AST (D11, plan/03 §2): the language the compiler compiles.
//!
//! P1 has the six node kinds that need no local variables. `LocalRef`,
//! `Lambda`, `Let` and the rest arrive in P2 and P4.

use crate::heap::GcRef;
use crate::value::Value;

#[derive(Debug)]
pub enum Expr {
    /// A literal: `quote`, or self-evaluating data.
    Const(Value),
    /// A global variable, already resolved to its binding cell.
    GlobalRef(GcRef),
    /// A top-level `(define name expr)`.
    GlobalDefine(GcRef, Box<Expr>),
    /// `(if test then else)`. A missing else is `Const(UNSPECIFIED)`.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `(begin e ...)`, never empty.
    Seq(Vec<Expr>),
    /// A procedure call: the operator and the operands.
    Call(Box<Expr>, Vec<Expr>),
}
