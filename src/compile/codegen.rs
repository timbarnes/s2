//! Code generation: core AST to a template.
//!
//! A post-order walk. Each expression's code leaves exactly one value on the
//! stack: operands are pushed left to right, then the instruction that
//! consumes them follows. `if` is the only construct that needs jumps, and
//! their targets are patched once the code they jump over has been emitted.

use crate::compile::ast::Expr;
use crate::value::Value;
use crate::vm::{Op, Template};

/// Compile a top-level form into a template of no arguments.
pub fn codegen(e: &Expr) -> Template {
    let mut g = Gen { code: Vec::new(), consts: Vec::new() };
    g.expr(e);
    g.code.push(Op::Return);
    Template { name: "top-level".into(), code: g.code, consts: g.consts }
}

struct Gen {
    code: Vec<Op>,
    consts: Vec<Value>,
}

impl Gen {
    /// The pool index of `v`, adding it if it isn't there. Constants are
    /// shared by identity (`eq?`), so a cell or symbol appears once.
    fn constant(&mut self, v: Value) -> u32 {
        let k = self.consts.iter().position(|&c| c.eq(v)).unwrap_or_else(|| {
            self.consts.push(v);
            self.consts.len() - 1
        });
        k as u32
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.push(op);
        self.code.len() - 1
    }

    /// Point the jump at `at` to the next instruction to be emitted.
    fn patch(&mut self, at: usize) {
        let here = self.code.len() as u32;
        match &mut self.code[at] {
            Op::Jump(t) | Op::JumpIfFalse(t) => *t = here,
            op => unreachable!("patching {:?}", op),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Const(v) => self.constant_op(*v),
            Expr::GlobalRef(cell) => {
                let k = self.constant(Value::obj(*cell));
                self.emit(Op::GlobalRef(k));
            }
            Expr::GlobalDefine(cell, value) => {
                self.expr(value);
                let k = self.constant(Value::obj(*cell));
                self.emit(Op::GlobalDefine(k));
            }
            Expr::If(test, then, els) => {
                self.expr(test);
                let to_else = self.emit(Op::JumpIfFalse(0));
                self.expr(then);
                let to_end = self.emit(Op::Jump(0));
                self.patch(to_else);
                self.expr(els);
                self.patch(to_end);
            }
            Expr::Seq(body) => {
                for (i, e) in body.iter().enumerate() {
                    if i > 0 {
                        self.emit(Op::Pop);
                    }
                    self.expr(e);
                }
            }
            Expr::Call(f, args) => {
                self.expr(f);
                for a in args {
                    self.expr(a);
                }
                self.emit(Op::Call(args.len() as u16));
            }
        }
    }

    /// Push a literal, with a dedicated instruction where there is one.
    fn constant_op(&mut self, v: Value) {
        let op = match v.as_fixnum().and_then(|n| i32::try_from(n).ok()) {
            Some(n) => Op::Fixnum(n),
            None if v.is_nil() => Op::Nil,
            None if v.is_unspecified() => Op::Unspecified,
            None => match v.as_bool() {
                Some(true) => Op::True,
                Some(false) => Op::False,
                None => Op::Const(self.constant(v)),
            },
        };
        self.emit(op);
    }
}
