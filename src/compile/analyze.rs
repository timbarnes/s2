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

use rustc_hash::FxHashSet;

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
/// or closure (03 §7). Closures and continuations copy values (a flat
/// closure copies its free variables, and `call/cc` copies the stack), so a
/// variable needs a box when a copy could go stale or be taken too early:
///
/// - **assigned** with `set!`. Without a box, a closure's copy and the
///   variable would go their separate ways after the `set!`, and so would
///   a continuation's copy of the stack slot: re-entering it would bring
///   back the value the variable had when the continuation was captured.
///   (P4 boxed only variables that were also captured by a closure, which
///   was right until continuations could be re-entered, in P7.)
/// - **referenced before it is initialised**: a `letrec` variable used in
///   its own init or an earlier one, such as a recursive procedure
///   referring to itself. The closure is made while the variable is still
///   undefined, so it must capture the box, which is filled in afterwards.
///   A reference that does run too early then finds the box empty, and
///   `BoxRef` reports the error.
///
/// A `letrec` variable used only after its init (in later inits or the
/// body) needs no box: its init is an ordinary assignment to a stack slot,
/// before anything can copy it. Re-entering a continuation captured during
/// a later init repeats the inits after it, so a copied slot is right.
///
/// Nor does a member of a **lambda group**: a `letrec` whose inits are all
/// lambdas, none of whose variables is assigned (`lambda_group`). No init
/// runs any code, so nothing can use a member early. Codegen makes the
/// closures in order, then patches the ones that captured a member made
/// after them (*Fixing Letrec*, P10). A named `let` is such a group, so its
/// loop variable is no longer a box.
pub fn boxed_variables(e: &Expr) -> FxHashSet<VarId> {
    let mut b = Boxing::default();
    b.collect_assigned(e);
    b.walk(e);
    b.assigned.extend(b.early);
    b.assigned
}

/// Whether a `letrec`'s bindings are a lambda group (see `boxed_variables`).
pub fn lambda_group(bindings: &[(crate::compile::ast::Var, Expr)], boxed: &FxHashSet<VarId>) -> bool {
    bindings.iter().all(|(v, init)| matches!(init, Expr::Lambda(_)) && !boxed.contains(&v.id))
}

#[derive(Default)]
struct Boxing {
    assigned: FxHashSet<VarId>,
    /// `letrec` variables whose init hasn't run yet, at this point of the
    /// walk.
    pending: FxHashSet<VarId>,
    early: FxHashSet<VarId>,
}

impl Boxing {
    /// The first pass: every variable that `set!` assigns.
    fn collect_assigned(&mut self, e: &Expr) {
        match e {
            Expr::Const(_) | Expr::GlobalRef(_) | Expr::LocalRef(_) => {}
            Expr::LocalSet(id, value) => {
                self.assigned.insert(*id);
                self.collect_assigned(value);
            }
            Expr::GlobalDefine(_, value) | Expr::GlobalSet(_, value) => self.collect_assigned(value),
            Expr::If(a, b, c) => [a, b, c].iter().for_each(|x| self.collect_assigned(x)),
            Expr::Seq(body) => body.iter().for_each(|x| self.collect_assigned(x)),
            Expr::Lambda(l) => self.collect_assigned(&l.body),
            Expr::Call(f, args) => {
                self.collect_assigned(f);
                args.iter().for_each(|x| self.collect_assigned(x));
            }
            Expr::Let(bindings, body) | Expr::Letrec(bindings, body) => {
                bindings.iter().for_each(|(_, init)| self.collect_assigned(init));
                self.collect_assigned(body);
            }
        }
    }

    fn used(&mut self, id: VarId) {
        if self.pending.contains(&id) {
            self.early.insert(id);
        }
    }

    fn walk(&mut self, e: &Expr) {
        match e {
            Expr::Const(_) | Expr::GlobalRef(_) => {}
            Expr::LocalRef(id) => self.used(*id),
            Expr::LocalSet(id, value) => {
                self.used(*id);
                self.assigned.insert(*id);
                self.walk(value);
            }
            Expr::GlobalDefine(_, value) | Expr::GlobalSet(_, value) => self.walk(value),
            Expr::If(test, then, els) => {
                self.walk(test);
                self.walk(then);
                self.walk(els);
            }
            Expr::Seq(body) => body.iter().for_each(|e| self.walk(e)),
            Expr::Lambda(l) => self.walk(&l.body),
            Expr::Call(f, args) => {
                self.walk(f);
                args.iter().for_each(|e| self.walk(e));
            }
            Expr::Let(bindings, body) => {
                bindings.iter().for_each(|(_, init)| self.walk(init));
                self.walk(body);
            }
            Expr::Letrec(bindings, body) => {
                // In a lambda group nothing runs during the inits, so no
                // reference is early.
                if !lambda_group(bindings, &self.assigned) {
                    self.pending.extend(bindings.iter().map(|(v, _)| v.id));
                }
                for (var, init) in bindings {
                    self.walk(init);
                    self.pending.remove(&var.id);
                }
                self.walk(body);
            }
        }
    }
}
