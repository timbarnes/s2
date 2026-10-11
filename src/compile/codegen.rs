//! Code generation: core AST to templates.
//!
//! A post-order walk. Each expression's code leaves exactly one value on the
//! stack: operands are pushed left to right, then the instruction that
//! consumes them follows.
//!
//! Two things are tracked during the walk:
//!
//! - **`depth`**, the number of values in the current frame (from `fp`).
//!   A `let` variable is simply the stack slot its init's value landed in,
//!   so its slot number is the depth when the init started.
//! - **`tail`**, whether the expression's value is the procedure's return
//!   value. A call in tail position becomes `TailCall`; any other
//!   expression in tail position is followed by `Return`. `if`, `begin` and
//!   `let` pass `tail` on to their last subexpressions (R7RS §3.5).

use rustc_hash::FxHashMap;

use crate::compile::analyze::free_variables;
use crate::compile::ast::{Expr, Lambda, Var, VarId};
use crate::heap::{GcRef, Heap};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::{Op, Template};

/// Compile a top-level form into a template of no arguments.
pub fn codegen(heap: &mut Heap, e: &Expr) -> GcRef {
    let mut g = Gen::new(heap);
    g.expr(e, true);
    g.finish("top-level", 0, false)
}

/// Where a local variable lives, from the point of view of the procedure
/// being compiled.
#[derive(Clone, Copy)]
enum Loc {
    /// A slot in the current frame: a parameter or a `let` variable.
    Local(u16),
    /// A free variable, copied into the running closure.
    Free(u16),
}

struct Gen<'h> {
    heap: &'h mut Heap,
    code: Vec<Op>,
    consts: Vec<Value>,
    depth: usize,
    locs: FxHashMap<VarId, Loc>,
    names: FxHashMap<VarId, String>,
    local_names: Vec<String>,
    free_names: Vec<String>,
}

impl<'h> Gen<'h> {
    fn new(heap: &'h mut Heap) -> Gen<'h> {
        Gen {
            heap,
            code: Vec::new(),
            consts: Vec::new(),
            depth: 0,
            locs: FxHashMap::default(),
            names: FxHashMap::default(),
            local_names: Vec::new(),
            free_names: Vec::new(),
        }
    }

    fn finish(self, name: &str, required: usize, rest: bool) -> GcRef {
        let t = Template {
            name: name.into(),
            required: required as u16,
            rest,
            nfree: self.free_names.len() as u16,
            code: self.code,
            consts: self.consts,
            local_names: self.local_names,
            free_names: self.free_names,
        };
        self.heap.template(t)
    }

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

    /// Return the value just pushed, if this expression is in tail position.
    fn ret(&mut self, tail: bool) {
        if tail {
            self.emit(Op::Return);
        }
    }

    fn bind_local(&mut self, var: Var, slot: usize) {
        let name = print_value(var.name);
        self.locs.insert(var.id, Loc::Local(slot as u16));
        if self.local_names.len() <= slot {
            self.local_names.resize(slot + 1, String::new());
        }
        let names = &mut self.local_names[slot];
        if names.is_empty() {
            *names = name.clone();
        } else if !names.split('/').any(|n| n == name) {
            names.push('/');
            names.push_str(&name);
        }
        self.names.insert(var.id, name);
    }

    /// Push the value of local variable `id`.
    fn load(&mut self, id: VarId) {
        match self.locs[&id] {
            Loc::Local(i) => self.emit(Op::LocalRef(i)),
            Loc::Free(i) => self.emit(Op::FreeRef(i)),
        };
        self.depth += 1;
    }

    fn expr(&mut self, e: &Expr, tail: bool) {
        match e {
            Expr::Const(v) => {
                self.constant_op(*v);
                self.depth += 1;
                self.ret(tail);
            }
            Expr::LocalRef(id) => {
                self.load(*id);
                self.ret(tail);
            }
            Expr::GlobalRef(cell) => {
                let k = self.constant(Value::obj(*cell));
                self.emit(Op::GlobalRef(k));
                self.depth += 1;
                self.ret(tail);
            }
            Expr::GlobalDefine(cell, value) => {
                self.expr(value, false);
                let k = self.constant(Value::obj(*cell));
                self.emit(Op::GlobalDefine(k));
                self.ret(tail);
            }
            Expr::If(test, then, els) => {
                self.expr(test, false);
                let to_else = self.emit(Op::JumpIfFalse(0));
                self.depth -= 1;
                let depth = self.depth;
                self.expr(then, tail);
                self.depth = depth;
                if tail {
                    // The then-branch has returned, so no jump over the else.
                    self.patch(to_else);
                    self.expr(els, true);
                } else {
                    let to_end = self.emit(Op::Jump(0));
                    self.patch(to_else);
                    self.expr(els, false);
                    self.patch(to_end);
                }
            }
            Expr::Seq(body) => {
                let (last, init) = body.split_last().unwrap();
                for e in init {
                    self.expr(e, false);
                    self.emit(Op::Pop);
                    self.depth -= 1;
                }
                self.expr(last, tail);
            }
            Expr::Lambda(l) => {
                let free = free_variables(l);
                for &id in &free {
                    self.load(id);
                }
                let t = self.lambda(l, &free);
                let k = self.constant(Value::obj(t));
                self.emit(Op::MakeClosure(k));
                self.depth = self.depth - free.len() + 1;
                self.ret(tail);
            }
            Expr::Call(f, args) => {
                self.expr(f, false);
                for a in args {
                    self.expr(a, false);
                }
                let n = args.len() as u16;
                self.emit(if tail { Op::TailCall(n) } else { Op::Call(n) });
                self.depth -= args.len();
            }
            Expr::Let(bindings, body) => {
                let base = self.depth;
                for (_, init) in bindings {
                    self.expr(init, false);
                }
                for (i, (var, _)) in bindings.iter().enumerate() {
                    self.bind_local(*var, base + i);
                }
                self.expr(body, tail);
                if !tail && !bindings.is_empty() {
                    self.emit(Op::Slide(bindings.len() as u16));
                    self.depth -= bindings.len();
                }
            }
        }
    }

    /// Compile an inner lambda into its own template. `free` lists its free
    /// variables; inside it they are `FreeRef 0`, `FreeRef 1`, ….
    fn lambda(&mut self, l: &Lambda, free: &[VarId]) -> GcRef {
        let mut g = Gen::new(self.heap);
        for (i, p) in l.params.iter().chain(&l.rest).enumerate() {
            g.bind_local(*p, i);
        }
        for (i, id) in free.iter().enumerate() {
            let name = self.names[id].clone();
            g.locs.insert(*id, Loc::Free(i as u16));
            g.free_names.push(name.clone());
            g.names.insert(*id, name);
        }
        g.depth = l.params.len() + l.rest.is_some() as usize;
        g.expr(&l.body, true);
        g.finish(&l.name, l.params.len(), l.rest.is_some())
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
