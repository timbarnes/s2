//! A minimal, hard-coded "expander": datum to core AST.
//!
//! It recognises `quote`, `if`, `define`, `begin`, `lambda` and `let` by
//! name, and treats any other list as a call. A symbol is a local variable
//! if a `lambda` or `let` around it binds it, and a global otherwise. Each
//! local binding gets a fresh `VarId`, so the passes after this one never
//! see names.
//!
//! ponytail: no macros and no hygiene. A local variable named like a keyword
//! does shadow it, but a global can't. The real expander, with a syntactic
//! environment, is P5. Internal `define` comes in P4, with `letrec*`.

use crate::compile::ast::{Expr, Lambda, Var, VarId};
use crate::globals::Env;
use crate::heap::{GcRef, Heap, Obj, list_to_vec};
use crate::printer::print_value;
use crate::value::Value;

/// Expand a top-level form.
pub fn expand(heap: &mut Heap, env: &mut Env, x: Value) -> Result<Expr, String> {
    Expander { heap, env, scope: Vec::new(), next_id: 0 }.expr(x, true)
}

struct Expander<'a> {
    heap: &'a mut Heap,
    env: &'a mut Env,
    /// The local variables in scope, innermost last.
    scope: Vec<(GcRef, VarId)>,
    next_id: VarId,
}

fn symbol(v: Value) -> Option<GcRef> {
    matches!(v.as_obj().map(|o| &o.kind), Some(Obj::Symbol(_))).then(|| v.as_ref().unwrap())
}

impl Expander<'_> {
    fn local(&self, sym: GcRef) -> Option<VarId> {
        self.scope.iter().rev().find(|(s, _)| *s == sym).map(|&(_, id)| id)
    }

    fn fresh(&mut self, name: GcRef) -> Var {
        self.next_id += 1;
        Var { id: self.next_id, name: Value::obj(name) }
    }

    /// `top` is true for a top-level form, or a `begin` at top level: the
    /// only places `define` is allowed until P4.
    fn expr(&mut self, x: Value, top: bool) -> Result<Expr, String> {
        let Some(obj) = x.as_ref() else {
            return if x.is_nil() { Err("empty combination: ()".into()) } else { Ok(Expr::Const(x)) };
        };
        match &obj.get().kind {
            Obj::Symbol(_) => Ok(match self.local(obj) {
                Some(id) => Expr::LocalRef(id),
                None => Expr::GlobalRef(self.env.cell(self.heap, obj)),
            }),
            Obj::Pair { car, .. } => {
                let items = list_to_vec(x).ok_or_else(|| format!("improper list in code: {}", print_value(x)))?;
                let keyword = match symbol(car.get()) {
                    Some(s) if self.local(s).is_none() => match &s.get().kind {
                        Obj::Symbol(name) => &**name,
                        _ => unreachable!(),
                    },
                    _ => "",
                };
                match keyword {
                    "quote" => self.quote(&items),
                    "if" => self.if_(&items),
                    "define" if top => self.define(&items),
                    "define" => Err(format!("define: only allowed at top level: {}", print_value(x))),
                    "begin" => self.begin(&items, top),
                    "lambda" if items.len() >= 3 => self.lambda("", items[1], &items[2..]),
                    "lambda" => Err(syntax_error("lambda", &items)),
                    "let" => self.let_(&items),
                    _ => self.call(&items),
                }
            }
            // Strings, vectors, bytevectors and numbers evaluate to themselves.
            _ => Ok(Expr::Const(x)),
        }
    }

    fn quote(&mut self, items: &[Value]) -> Result<Expr, String> {
        match items {
            [_, datum] => Ok(Expr::Const(*datum)),
            _ => Err(syntax_error("quote", items)),
        }
    }

    fn if_(&mut self, items: &[Value]) -> Result<Expr, String> {
        let (test, then, els) = match items {
            [_, test, then] => (test, then, None),
            [_, test, then, els] => (test, then, Some(els)),
            _ => return Err(syntax_error("if", items)),
        };
        let els = match els {
            Some(&e) => self.expr(e, false)?,
            None => Expr::Const(Value::UNSPECIFIED),
        };
        Ok(Expr::If(Box::new(self.expr(*test, false)?), Box::new(self.expr(*then, false)?), Box::new(els)))
    }

    /// `(define name expr)` or `(define (name . formals) body ...)`.
    fn define(&mut self, items: &[Value]) -> Result<Expr, String> {
        if let [_, target, ..] = items
            && let Some(Obj::Pair { car, cdr }) = target.as_obj().map(|o| &o.kind)
            && let Some(name) = symbol(car.get())
            && items.len() >= 3
        {
            let lambda = self.lambda(&print_value(Value::obj(name)), cdr.get(), &items[2..])?;
            return Ok(Expr::GlobalDefine(self.env.cell(self.heap, name), Box::new(lambda)));
        }
        let [_, name, value] = items else { return Err(syntax_error("define", items)) };
        let Some(name) = symbol(*name) else { return Err(syntax_error("define", items)) };
        let mut value = self.expr(*value, false)?;
        if let Expr::Lambda(l) = &mut value
            && l.name.is_empty()
        {
            l.name = print_value(Value::obj(name)).into();
        }
        Ok(Expr::GlobalDefine(self.env.cell(self.heap, name), Box::new(value)))
    }

    fn begin(&mut self, items: &[Value], top: bool) -> Result<Expr, String> {
        if items.len() == 1 && top {
            // `(begin)` is only legal at top level; R7RS leaves its value
            // unspecified.
            return Ok(Expr::Const(Value::UNSPECIFIED));
        }
        self.body(&items[1..], top).map_err(|_| "begin: empty in an expression".into())
    }

    /// A sequence of expressions: one expression, or a `Seq`.
    fn body(&mut self, forms: &[Value], top: bool) -> Result<Expr, String> {
        let mut body = forms.iter().map(|&x| self.expr(x, top)).collect::<Result<Vec<_>, _>>()?;
        match body.len() {
            0 => Err("empty body".into()),
            1 => Ok(body.pop().unwrap()),
            _ => Ok(Expr::Seq(body)),
        }
    }

    /// `(lambda formals body ...)`. `formals` is a list of symbols, possibly
    /// improper (`(a b . rest)`), or a single symbol for all the arguments.
    fn lambda(&mut self, name: &str, formals: Value, body: &[Value]) -> Result<Expr, String> {
        let mut params = Vec::new();
        let mut tail = formals;
        while let Some(Obj::Pair { car, cdr }) = tail.as_obj().map(|o| &o.kind) {
            let s = symbol(car.get()).ok_or_else(|| format!("lambda: bad parameter: {}", print_value(car.get())))?;
            params.push(self.fresh(s));
            tail = cdr.get();
        }
        let rest = match symbol(tail) {
            Some(s) => Some(self.fresh(s)),
            None if tail.is_nil() => None,
            None => return Err(format!("lambda: bad parameter list: {}", print_value(formals))),
        };
        let all: Vec<Var> = params.iter().chain(&rest).copied().collect();
        for (i, v) in all.iter().enumerate() {
            if all[..i].iter().any(|w| w.name.eq(v.name)) {
                return Err(format!("lambda: duplicate parameter: {}", print_value(v.name)));
            }
        }
        let body = self.scoped(&all, |e| e.body(body, false))?;
        Ok(Expr::Lambda(Box::new(Lambda { name: name.into(), params, rest, body })))
    }

    /// `(let ((v init) ...) body ...)`.
    fn let_(&mut self, items: &[Value]) -> Result<Expr, String> {
        if items.len() < 3 {
            return Err(syntax_error("let", items));
        }
        if symbol(items[1]).is_some() {
            return Err("named let: not supported yet (P4)".into());
        }
        let specs = list_to_vec(items[1]).ok_or_else(|| syntax_error("let", items))?;
        let mut bindings = Vec::new();
        for spec in specs {
            match list_to_vec(spec).as_deref() {
                Some(&[name, init]) if symbol(name).is_some() => {
                    // The inits are expanded outside the new scope.
                    let init = self.expr(init, false)?;
                    let var = self.fresh(symbol(name).unwrap());
                    if bindings.iter().any(|(w, _): &(Var, Expr)| w.name.eq(var.name)) {
                        return Err(format!("let: duplicate variable: {}", print_value(name)));
                    }
                    bindings.push((var, init));
                }
                _ => return Err(format!("let: bad binding: {}", print_value(spec))),
            }
        }
        let vars: Vec<Var> = bindings.iter().map(|(v, _)| *v).collect();
        let body = self.scoped(&vars, |e| e.body(&items[2..], false))?;
        Ok(Expr::Let(bindings, Box::new(body)))
    }

    /// Run `f` with `vars` in scope.
    fn scoped(&mut self, vars: &[Var], f: impl FnOnce(&mut Self) -> Result<Expr, String>) -> Result<Expr, String> {
        let depth = self.scope.len();
        self.scope.extend(vars.iter().map(|v| (v.name.as_ref().unwrap(), v.id)));
        let result = f(self);
        self.scope.truncate(depth);
        result
    }

    fn call(&mut self, items: &[Value]) -> Result<Expr, String> {
        let f = self.expr(items[0], false)?;
        let args = items[1..].iter().map(|&x| self.expr(x, false)).collect::<Result<Vec<_>, _>>()?;
        Ok(Expr::Call(Box::new(f), args))
    }
}

fn syntax_error(keyword: &str, items: &[Value]) -> String {
    format!("{}: bad syntax: ({})", keyword, items.iter().map(|&v| print_value(v)).collect::<Vec<_>>().join(" "))
}
