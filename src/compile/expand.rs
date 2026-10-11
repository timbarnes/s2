//! A minimal, hard-coded "expander": datum to core AST.
//!
//! It recognises `quote`, `if`, `define` and `begin` by name, and treats any
//! other list as a call. A symbol is a global variable.
//!
//! ponytail: no macros, no hygiene, no local scope, so these keywords can't
//! be shadowed. The real expander, with a syntactic environment, is P5.

use crate::compile::ast::Expr;
use crate::globals::Env;
use crate::heap::{Heap, Obj, list_to_vec};
use crate::printer::print_value;
use crate::value::Value;

/// Expand a top-level form.
pub fn expand(heap: &mut Heap, env: &mut Env, x: Value) -> Result<Expr, String> {
    Expander { heap, env }.expr(x, true)
}

struct Expander<'a> {
    heap: &'a mut Heap,
    env: &'a mut Env,
}

impl Expander<'_> {
    /// `top` is true for a top-level form, or a `begin` at top level: the
    /// only places P1 allows `define`.
    fn expr(&mut self, x: Value, top: bool) -> Result<Expr, String> {
        let Some(obj) = x.as_ref() else {
            return if x.is_nil() { Err("empty combination: ()".into()) } else { Ok(Expr::Const(x)) };
        };
        match &obj.get().kind {
            Obj::Symbol(_) => Ok(Expr::GlobalRef(self.env.cell(self.heap, obj))),
            Obj::Pair { car, .. } => {
                let items = list_to_vec(x).ok_or_else(|| format!("improper list in code: {}", print_value(x)))?;
                let keyword = match car.get().as_obj().map(|o| &o.kind) {
                    Some(Obj::Symbol(s)) => &**s,
                    _ => "",
                };
                match keyword {
                    "quote" => self.quote(&items),
                    "if" => self.if_(&items),
                    "define" if top => self.define(&items),
                    "define" => Err(format!("define: only allowed at top level: {}", print_value(x))),
                    "begin" => self.begin(&items, top),
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

    fn define(&mut self, items: &[Value]) -> Result<Expr, String> {
        match items {
            [_, name, value] if matches!(name.as_obj().map(|o| &o.kind), Some(Obj::Symbol(_))) => {
                let cell = self.env.cell(self.heap, name.as_ref().unwrap());
                Ok(Expr::GlobalDefine(cell, Box::new(self.expr(*value, false)?)))
            }
            _ => Err(syntax_error("define", items)),
        }
    }

    fn begin(&mut self, items: &[Value], top: bool) -> Result<Expr, String> {
        let body = items[1..].iter().map(|&x| self.expr(x, top)).collect::<Result<Vec<_>, _>>()?;
        Ok(match body.len() {
            // `(begin)` is only legal at top level; R7RS leaves its value
            // unspecified.
            0 if top => Expr::Const(Value::UNSPECIFIED),
            0 => return Err("begin: empty in an expression".into()),
            1 => body.into_iter().next().unwrap(),
            _ => Expr::Seq(body),
        })
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
