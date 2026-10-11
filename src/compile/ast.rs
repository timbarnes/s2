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

/// The core AST as text, for `--expand`. A local variable is shown as
/// `name.id`, so two variables with the same name (from shadowing, or a
/// macro's renamed temporaries) can be told apart. A named procedure is
/// shown as `(named-lambda (name param ...) body)`, as in MIT Scheme.
pub fn show(e: &Expr) -> String {
    let mut out = String::new();
    show_into(&mut out, e, &mut rustc_hash::FxHashMap::default());
    out
}

fn show_into(out: &mut String, e: &Expr, names: &mut rustc_hash::FxHashMap<VarId, String>) {
    use crate::globals::cell_name;
    use crate::printer::print_value;
    let bind = |names: &mut rustc_hash::FxHashMap<VarId, String>, v: &Var| {
        let s = format!("{}.{}", print_value(v.name), v.id);
        names.insert(v.id, s.clone());
        s
    };
    match e {
        Expr::Const(v) if v.is_unspecified() => out.push_str("#<unspecified>"),
        Expr::Const(v) => match v.as_obj().map(|o| &o.kind) {
            Some(crate::heap::Obj::Symbol(_) | crate::heap::Obj::Pair { .. }) => {
                out.push('\'');
                out.push_str(&print_value(*v));
            }
            _ if v.is_nil() => out.push_str("'()"),
            _ => out.push_str(&print_value(*v)),
        },
        Expr::LocalRef(id) => out.push_str(&names[id]),
        Expr::GlobalRef(cell) => out.push_str(&print_value(cell_name(*cell))),
        Expr::LocalSet(id, v) => {
            out.push_str(&format!("(set! {} ", names[id]));
            show_into(out, v, names);
            out.push(')');
        }
        Expr::GlobalSet(cell, v) | Expr::GlobalDefine(cell, v) => {
            let keyword = if matches!(e, Expr::GlobalSet(..)) { "set!" } else { "define" };
            out.push_str(&format!("({} {} ", keyword, print_value(cell_name(*cell))));
            show_into(out, v, names);
            out.push(')');
        }
        Expr::If(a, b, c) => {
            out.push_str("(if");
            for x in [a, b, c] {
                out.push(' ');
                show_into(out, x, names);
            }
            out.push(')');
        }
        Expr::Seq(es) => {
            out.push_str("(begin");
            for x in es {
                out.push(' ');
                show_into(out, x, names);
            }
            out.push(')');
        }
        Expr::Lambda(l) => {
            let named = (!l.name.is_empty()).then(|| l.name.to_string());
            let head: Vec<String> = named.into_iter().chain(l.params.iter().map(|p| bind(names, p))).collect();
            let formals = match &l.rest {
                Some(r) if head.is_empty() => bind(names, r),
                Some(r) => format!("({} . {})", head.join(" "), bind(names, r)),
                None => format!("({})", head.join(" ")),
            };
            let keyword = if l.name.is_empty() { "lambda" } else { "named-lambda" };
            out.push_str(&format!("({} {} ", keyword, formals));
            show_into(out, &l.body, names);
            out.push(')');
        }
        Expr::Call(f, args) => {
            out.push('(');
            show_into(out, f, names);
            for a in args {
                out.push(' ');
                show_into(out, a, names);
            }
            out.push(')');
        }
        Expr::Let(bindings, body) | Expr::Letrec(bindings, body) => {
            let rec = matches!(e, Expr::Letrec(..));
            out.push_str(if rec { "(letrec* (" } else { "(let (" });
            // A let's inits are outside its scope, so they are shown before
            // its variables are named; a letrec's are inside.
            let vars: Vec<String> = if rec { bindings.iter().map(|(v, _)| bind(names, v)).collect() } else { Vec::new() };
            for (i, (v, init)) in bindings.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let mut init_text = String::new();
                show_into(&mut init_text, init, names);
                let var = if rec { vars[i].clone() } else { bind(names, v) };
                out.push_str(&format!("({} {})", var, init_text));
            }
            out.push_str(") ");
            show_into(out, body, names);
            out.push(')');
        }
    }
}
