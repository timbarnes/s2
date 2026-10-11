//! Analysis over the core AST (plan/03 §1).
//!
//! P2 needs one fact: the free variables of each lambda, which are the
//! values a flat closure copies when it is made. Tail positions are decided
//! during code generation, which already walks the tree knowing whether
//! the current expression's value is returned (`tail` in codegen.rs).
//! Assigned-variable analysis joins this file in P4.

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
        Expr::GlobalDefine(_, value) => walk(value, bound, refs),
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
    }
}
