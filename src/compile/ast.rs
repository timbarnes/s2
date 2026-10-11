//! The core AST (D11, plan/03 §2): the language the compiler compiles.
//!
//! P2 added local variables: `LocalRef`, `Lambda` and `Let`. P4 adds
//! assignment (`LocalSet`, `GlobalSet`) and `Letrec`.

use crate::heap::GcRef;
use crate::value::Value;

/// A local variable's identity. The expander gives every binding a fresh
/// id, so two variables with the same name are never confused, and later
/// passes never need to think about scope or shadowing.
pub type VarId = u32;

/// A binding occurrence of a local variable: its id, and its name (a symbol)
/// for the disassembler.
#[derive(Clone, Copy, Debug)]
pub struct Var {
    pub id: VarId,
    pub name: Value,
}

#[derive(Debug)]
pub enum Expr {
    /// A literal: `quote`, or self-evaluating data.
    Const(Value),
    /// A local variable: a lambda parameter or a `let` variable.
    LocalRef(VarId),
    /// `(set! v expr)` on a local variable.
    LocalSet(VarId, Box<Expr>),
    /// A global variable, already resolved to its binding cell.
    GlobalRef(GcRef),
    /// `(set! v expr)` on a global variable. An error if it is unbound.
    GlobalSet(GcRef, Box<Expr>),
    /// A top-level `(define name expr)`.
    GlobalDefine(GcRef, Box<Expr>),
    /// `(if test then else)`. A missing else is `Const(UNSPECIFIED)`.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `(begin e ...)`, never empty.
    Seq(Vec<Expr>),
    Lambda(Box<Lambda>),
    /// A procedure call: the operator and the operands.
    Call(Box<Expr>, Vec<Expr>),
    /// `(let ((v init) ...) body)`. The same meaning as
    /// `((lambda (v ...) body) init ...)`, but compiled without making a
    /// closure: the variables are stack slots in the current frame.
    Let(Vec<(Var, Expr)>, Box<Expr>),
    /// `(letrec* ((v init) ...) body)`: the variables are in scope in the
    /// inits too. Each starts out undefined, and the inits are evaluated
    /// and assigned in order. `letrec`, internal `define`s and named `let`
    /// all become this.
    Letrec(Vec<(Var, Expr)>, Box<Expr>),
}

#[derive(Debug)]
pub struct Lambda {
    /// The name from `(define (name ...) ...)` or `(define name (lambda
    /// ...))`; empty for an anonymous procedure.
    pub name: Box<str>,
    pub params: Vec<Var>,
    /// The rest parameter, from `(lambda (a . rest) ...)` or
    /// `(lambda args ...)`.
    pub rest: Option<Var>,
    pub body: Expr,
}
