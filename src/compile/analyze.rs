//! Analysis over the core AST (plan/03 §1).
//!
//! Two facts:
//!
//! - the free variables of each lambda, which are the values a flat
//!   closure copies when it is made;
//! - which variables need boxes (`boxed_variables`).
//!
//! Tail positions are decided during code generation, which already walks
//! the tree knowing whether the current expression's value is returned
//! (`tail` in codegen.rs).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::ast::{Expr, Lambda, VarId};

/// The local variables that `lambda` references but doesn't bind, in order
/// of first reference.
///
/// Every binding has a unique `VarId`, so no scope tracking is needed: a
/// variable is free in `lambda` if it is referenced anywhere inside it
/// (including inside nested lambdas) and bound nowhere inside it.
// ponytail: recomputed for each nested lambda, so O(size × nesting depth).
// Annotate the tree in one pass if deep nesting ever makes this show up.
pub fn free_variables(lambda: &Lambda) -> Vec<VarId> {
    let mut bound: FxHashSet<VarId> = lambda.params.iter().chain(&lambda.rest).map(|v| v.id).collect();
    let mut refs = Vec::new();
    walk(&lambda.body, &mut bound, &mut refs);
    let mut seen = FxHashSet::default();
    refs.retain(|id| !bound.contains(id) && seen.insert(*id));
    refs
}

fn walk(e: &Expr, bound: &mut FxHashSet<VarId>, refs: &mut Vec<VarId>) {
    match e {
        Expr::Const(_) | Expr::GlobalRef(_) => {}
        Expr::LocalRef(id) => refs.push(*id),
        // Assigning a captured variable needs its box, so it counts as a
        // reference.
        Expr::LocalSet(id, value) => {
            refs.push(*id);
            walk(value, bound, refs);
        }
        Expr::GlobalDefine(_, value) | Expr::GlobalSet(_, value) => walk(value, bound, refs),
        Expr::If(test, then, els) => {
            walk(test, bound, refs);
            walk(then, bound, refs);
            walk(els, bound, refs);
        }
        Expr::Seq(body) => body.iter().for_each(|e| walk(e, bound, refs)),
        Expr::Lambda(l) => {
            bound.extend(l.params.iter().chain(&l.rest).map(|v| v.id));
            walk(&l.body, bound, refs);
        }
        Expr::Call(f, args) => {
            walk(f, bound, refs);
            args.iter().for_each(|e| walk(e, bound, refs));
        }
        Expr::Let(bindings, body) => {
            for (var, init) in bindings {
                bound.insert(var.id);
                walk(init, bound, refs);
            }
            walk(body, bound, refs);
        }
        Expr::Letrec(bindings, body) => {
            bound.extend(bindings.iter().map(|(v, _)| v.id));
            bindings.iter().for_each(|(_, init)| walk(init, bound, refs));
            walk(body, bound, refs);
        }
    }
}

/// The variables that live in a box rather than directly in a stack slot
/// or closure (03 §7). A flat closure copies values, so a variable needs a
/// box when a copy could go stale or be taken too early:
///
/// - **assigned and captured**: `set!` on it, and a reference from a lambda
///   other than the one binding it. Without a box, the closure's copy and
///   the variable would go their separate ways after the `set!`.
/// - **referenced before it is initialised**: a `letrec` variable used in
///   its own init or an earlier one, such as a recursive procedure
///   referring to itself. The closure is made while the variable is still
///   undefined, so it must capture the box, which is filled in afterwards.
///   A reference that does run too early then finds the box empty, and
///   `BoxRef` reports the error.
///
/// A `letrec` variable used only after its init (in later inits or the
/// body) needs no box: its init is an ordinary assignment to a stack slot,
/// before anything can copy it.
pub fn boxed_variables(e: &Expr) -> FxHashSet<VarId> {
    let mut b = Boxing::default();
    b.walk(e, 0);
    let mut boxed: FxHashSet<VarId> = b.assigned.intersection(&b.captured).copied().collect();
    boxed.extend(b.early);
    boxed
}

#[derive(Default)]
struct Boxing {
    /// The lambda binding each variable: 0 for the top-level form, then
    /// lambdas numbered as they are met.
    owner: FxHashMap<VarId, u32>,
    lambdas: u32,
    assigned: FxHashSet<VarId>,
    captured: FxHashSet<VarId>,
    /// `letrec` variables whose init hasn't run yet, at this point of the
    /// walk.
    pending: FxHashSet<VarId>,
    early: FxHashSet<VarId>,
}

impl Boxing {
    /// `id` is used (referenced or assigned) inside lambda `current`.
    fn used(&mut self, id: VarId, current: u32) {
        if self.owner[&id] != current {
            self.captured.insert(id);
        }
        if self.pending.contains(&id) {
            self.early.insert(id);
        }
    }

    fn walk(&mut self, e: &Expr, current: u32) {
        match e {
            Expr::Const(_) | Expr::GlobalRef(_) => {}
            Expr::LocalRef(id) => self.used(*id, current),
            Expr::LocalSet(id, value) => {
                self.used(*id, current);
                self.assigned.insert(*id);
                self.walk(value, current);
            }
            Expr::GlobalDefine(_, value) | Expr::GlobalSet(_, value) => self.walk(value, current),
            Expr::If(test, then, els) => {
                self.walk(test, current);
                self.walk(then, current);
                self.walk(els, current);
            }
            Expr::Seq(body) => body.iter().for_each(|e| self.walk(e, current)),
            Expr::Lambda(l) => {
                self.lambdas += 1;
                let me = self.lambdas;
                for p in l.params.iter().chain(&l.rest) {
                    self.owner.insert(p.id, me);
                }
                self.walk(&l.body, me);
            }
            Expr::Call(f, args) => {
                self.walk(f, current);
                args.iter().for_each(|e| self.walk(e, current));
            }
            Expr::Let(bindings, body) => {
                for (var, init) in bindings {
                    self.walk(init, current);
                    self.owner.insert(var.id, current);
                }
                self.walk(body, current);
            }
            Expr::Letrec(bindings, body) => {
                for (var, _) in bindings {
                    self.owner.insert(var.id, current);
                    self.pending.insert(var.id);
                }
                for (var, init) in bindings {
                    self.walk(init, current);
                    self.pending.remove(&var.id);
                }
                self.walk(body, current);
            }
        }
    }
}
