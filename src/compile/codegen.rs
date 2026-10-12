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
//!
//! A variable in `boxed` (`analyze::boxed_variables`) holds a box: `MakeBox`
//! where it is bound, `BoxRef` after each load, `BoxSet` to assign. A
//! closure capturing it copies the box itself.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::analyze::{boxed_variables, free_variables, lambda_group};
use crate::compile::ast::{Expr, Lambda, Var, VarId};
use crate::globals::{TopBinding, cell_name, cell_value, env_of};
use crate::heap::{GcRef, Heap, Obj};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::op::Test;
use crate::vm::{Op, Template};

/// The inlined instruction for a call to the global `cell` with `argc`
/// arguments, if it is one: `cell` must be the system environment's
/// binding of a standard procedure that has one. Those cells never change
/// (user code can't assign an imported name, and a `define` of one makes a
/// new cell), so the inlined code means exactly what the call does.
pub fn inline_op(heap: &Heap, cell: GcRef, argc: usize) -> Option<Op> {
    let Some(Obj::Primitive(p)) = cell_value(cell).get().as_obj().map(|o| &o.kind) else { return None };
    let name = cell_name(cell).as_ref()?;
    let system = env_of(heap.system?).borrow().lookup(name);
    if system != Some(TopBinding::Variable(cell)) {
        return None;
    }
    Op::inline(p.name, argc)
}

/// Compile a top-level form into a template of no arguments.
pub fn codegen(heap: &mut Heap, e: &Expr) -> GcRef {
    let boxed = boxed_variables(e);
    let mut g = Gen::new(heap, &boxed);
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
    boxed: &'h FxHashSet<VarId>,
    code: Vec<Op>,
    consts: Vec<Value>,
    depth: usize,
    locs: FxHashMap<VarId, Loc>,
    names: FxHashMap<VarId, String>,
    local_names: Vec<String>,
    free_names: Vec<String>,
    /// The lambda-group variable this procedure is bound to, if it is
    /// one: its own name, which always means this closure, so a tail call
    /// to it can be a `SelfTailCall`.
    self_var: Option<VarId>,
    /// Set while compiling a lambda group's init: the variable the lambda
    /// is bound to, handed to it as its `self_var`.
    next_self: Option<VarId>,
    /// The number of parameters of the procedure being compiled.
    params: usize,
}

impl<'h> Gen<'h> {
    fn new(heap: &'h mut Heap, boxed: &'h FxHashSet<VarId>) -> Gen<'h> {
        Gen {
            heap,
            boxed,
            code: Vec::new(),
            consts: Vec::new(),
            depth: 0,
            locs: FxHashMap::default(),
            names: FxHashMap::default(),
            local_names: Vec::new(),
            free_names: Vec::new(),
            self_var: None,
            next_self: None,
            params: 0,
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
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfNot(_, t) | Op::JumpIfNotImm(_, _, t) => *t = here,
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
        self.load_raw(id);
        if self.boxed.contains(&id) {
            self.emit(Op::BoxRef);
        }
    }

    /// Push what local variable `id`'s slot holds: its box, if it has one.
    fn load_raw(&mut self, id: VarId) {
        match self.locs[&id] {
            Loc::Local(i) => self.emit(Op::LocalRef(i)),
            Loc::Free(i) => self.emit(Op::FreeRef(i)),
        };
        self.depth += 1;
    }

    /// Box local variable `var`, bound in slot `slot`, if it needs a box.
    fn box_if_needed(&mut self, var: Var, slot: usize) {
        if self.boxed.contains(&var.id) {
            self.emit(Op::MakeBox(slot as u16));
        }
    }

    /// Assign the value of `value` to local variable `id`, leaving nothing
    /// on the stack.
    fn assign(&mut self, id: VarId, value: &Expr) {
        if self.boxed.contains(&id) {
            self.load_raw(id);
            self.expr(value, false);
            self.emit(Op::BoxSet);
            self.depth -= 2;
        } else {
            self.expr(value, false);
            match self.locs[&id] {
                Loc::Local(i) => self.emit(Op::LocalSet(i)),
                // Captured and assigned means boxed.
                Loc::Free(_) => unreachable!("assignment to an unboxed free variable"),
            };
            self.depth -= 1;
        }
    }

    /// Push the unspecified value, the result of an assignment.
    fn unspecified(&mut self, tail: bool) {
        self.emit(Op::Unspecified);
        self.depth += 1;
        self.ret(tail);
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
            Expr::LocalSet(id, value) => {
                self.assign(*id, value);
                self.unspecified(tail);
            }
            Expr::GlobalSet(cell, value) => {
                self.expr(value, false);
                let k = self.constant(Value::obj(*cell));
                self.emit(Op::GlobalSet(k));
                self.depth -= 1;
                self.unspecified(tail);
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
                // `(if (not x) a b)` is `(if x b a)`.
                let (mut test, mut then, mut els) = (&**test, then, els);
                while let Expr::Call(f, args) = test
                    && let Expr::GlobalRef(cell) = **f
                    && let Some(Op::Not) = self.inlinable(cell, args.len())
                {
                    test = &args[0];
                    (then, els) = (els, then);
                }
                // A comparison with a small constant.
                let imm = match self.fused_test(test) {
                    Some((t, [a, Expr::Const(k)])) if t.arity() == 2 && t != Test::EqP => {
                        k.as_fixnum().and_then(|k| i16::try_from(k).ok()).map(|k| (t, a, k))
                    }
                    _ => None,
                };
                let to_else = match (imm, self.fused_test(test)) {
                    (Some((t, a, k)), _) => {
                        self.expr(a, false);
                        self.depth -= 1;
                        self.emit(Op::JumpIfNotImm(t, k, 0))
                    }
                    (None, Some((t, args))) => {
                        for a in args {
                            self.expr(a, false);
                        }
                        self.depth -= args.len();
                        self.emit(Op::JumpIfNot(t, 0))
                    }
                    (None, None) => {
                        self.expr(test, false);
                        self.depth -= 1;
                        self.emit(Op::JumpIfFalse(0))
                    }
                };
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
                let self_var = self.next_self.take();
                let free = free_variables(l);
                for &id in &free {
                    self.load_raw(id);
                }
                let t = self.lambda(l, &free, self_var);
                let k = self.constant(Value::obj(t));
                self.emit(Op::MakeClosure(k));
                self.depth = self.depth - free.len() + 1;
                self.ret(tail);
            }
            // A loop: a tail call from a lambda-group procedure to itself.
            Expr::Call(f, args)
                if tail && matches!(**f, Expr::LocalRef(id) if Some(id) == self.self_var) && args.len() == self.params =>
            {
                for a in args {
                    self.expr(a, false);
                }
                self.emit(Op::SelfTailCall(args.len() as u16));
            }
            Expr::Call(f, args) => {
                let inline = match **f {
                    Expr::GlobalRef(cell) => self.inlinable(cell, args.len()),
                    _ => None,
                };
                if let Some(op) = inline {
                    if self.superinstruction(op, args, tail).is_some() {
                        return;
                    }
                    for a in args {
                        self.expr(a, false);
                    }
                    self.emit(op);
                    self.depth = self.depth + 1 - args.len();
                    self.ret(tail);
                    return;
                }
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
                    self.box_if_needed(*var, base + i);
                }
                self.end_scope(body, tail, bindings.len());
            }
            Expr::Letrec(bindings, body) => {
                // Every variable gets its slot (and box) first, undefined,
                // so the inits can capture them. Then each init is
                // evaluated and assigned in order (letrec*).
                let base = self.depth;
                for (i, (var, _)) in bindings.iter().enumerate() {
                    self.emit(Op::Undefined);
                    self.depth += 1;
                    self.bind_local(*var, base + i);
                    self.box_if_needed(*var, base + i);
                }
                let group = lambda_group(bindings, self.boxed);
                for (var, init) in bindings {
                    self.next_self = group.then_some(var.id);
                    self.assign(var.id, init);
                    self.next_self = None;
                }
                if group {
                    self.patch_group(bindings);
                }
                self.end_scope(body, tail, bindings.len());
            }
        }
    }

    /// Compile the inlined call `op` of `args` as a superinstruction, if it
    /// is one: `car` or `cdr` of an unboxed local, or adding or
    /// subtracting a constant.
    fn superinstruction(&mut self, op: Op, args: &[Expr], tail: bool) -> Option<()> {
        let small = |e: &Expr| match e {
            Expr::Const(k) => k.as_fixnum().and_then(|k| i32::try_from(k).ok()),
            _ => None,
        };
        match (op, args) {
            (Op::Car | Op::Cdr, [Expr::LocalRef(id)]) if !self.boxed.contains(id) => {
                let Loc::Local(i) = self.locs[id] else { return None };
                self.emit(if matches!(op, Op::Car) { Op::CarLocal(i) } else { Op::CdrLocal(i) });
                self.depth += 1;
            }
            (Op::Add, [a, b]) => {
                let (k, a) = match (small(b), small(a)) {
                    (Some(k), _) => (k, a),
                    (None, Some(k)) => (k, b),
                    _ => return None,
                };
                self.expr(a, false);
                self.emit(Op::AddImm(k));
            }
            (Op::Sub, [a, b]) => {
                let k = small(b)?;
                self.expr(a, false);
                self.emit(Op::SubImm(k));
            }
            _ => return None,
        }
        self.ret(tail);
        Some(())
    }

    /// The test and arguments of an `if` test that is an inlinable
    /// predicate or comparison, for `JumpIfNot`.
    fn fused_test<'e>(&self, test: &'e Expr) -> Option<(Test, &'e [Expr])> {
        let Expr::Call(f, args) = test else { return None };
        let Expr::GlobalRef(cell) = **f else { return None };
        Some((Test::of(self.inlinable(cell, args.len())?)?, args))
    }

    /// After a lambda group's closures are made, in order: each one that
    /// captured a member made at or after it captured a placeholder (the
    /// slot's `Undefined`), so fill in the member now.
    fn patch_group(&mut self, bindings: &[(Var, Expr)]) {
        for (i, (var, init)) in bindings.iter().enumerate() {
            let Expr::Lambda(l) = init else { unreachable!("a lambda group") };
            for (j, fv) in free_variables(l).iter().enumerate() {
                if let Some(k) = bindings.iter().position(|(v, _)| v.id == *fv)
                    && k >= i
                    && let (Loc::Local(c), Loc::Local(v)) = (self.locs[&var.id], self.locs[fv])
                {
                    self.emit(Op::PatchFree(c, j as u16, v));
                }
            }
        }
    }

    /// Compile the body of a `let` or `letrec` whose `n` variables are the
    /// top `n` slots, then drop them unless the frame is ending anyway.
    fn end_scope(&mut self, body: &Expr, tail: bool, n: usize) {
        self.expr(body, tail);
        if !tail && n > 0 {
            self.emit(Op::Slide(n as u16));
            self.depth -= n;
        }
    }

    /// The inlined instruction for a call to the global `cell` with `argc`
    /// arguments, if it is one (`inline_op`).
    fn inlinable(&self, cell: GcRef, argc: usize) -> Option<Op> {
        inline_op(self.heap, cell, argc)
    }

    /// Compile an inner lambda into its own template. `free` lists its free
    /// variables; inside it they are `FreeRef 0`, `FreeRef 1`, ….
    fn lambda(&mut self, l: &Lambda, free: &[VarId], self_var: Option<VarId>) -> GcRef {
        let mut g = Gen::new(self.heap, self.boxed);
        g.self_var = self_var.filter(|_| l.rest.is_none());
        g.params = l.params.len();
        for (i, p) in l.params.iter().chain(&l.rest).enumerate() {
            g.bind_local(*p, i);
            g.box_if_needed(*p, i);
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
