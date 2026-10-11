//! A minimal, hard-coded "expander": datum to core AST.
//!
//! It recognises `quote`, `if`, `define`, `set!`, `begin`, `lambda`, `let`
//! (and named `let`), `letrec` and `letrec*` by name, and treats any other
//! list as a call. A symbol is a local variable
//! if a `lambda` or `let` around it binds it, and a global otherwise. Each
//! local binding gets a fresh `VarId`, so the passes after this one never
//! see names.
//!
//! ponytail: no macros and no hygiene. A local variable named like a keyword
//! does shadow it, but a global can't. The real expander, with a syntactic
//! environment, is P5, and with it the full R7RS body rules (here, a body's
//! definitions must come first and can't come from a `begin`).

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

/// The value of a `define`, before expansion: `(define (name . formals)
/// body ...)` or `(define name expr)`.
enum Definition {
    Procedure(Value, Vec<Value>),
    Expr(Value),
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

    /// The keyword `head` names, or "" if it isn't a symbol or is a local
    /// variable (which shadows any keyword).
    fn keyword<'b>(&self, head: Value) -> &'b str {
        match symbol(head) {
            Some(s) if self.local(s).is_none() => match &s.get().kind {
                Obj::Symbol(name) => name,
                _ => unreachable!(),
            },
            _ => "",
        }
    }

    /// Whether `x` is a `(define ...)` form.
    fn is_definition(&self, x: Value) -> bool {
        matches!(x.as_obj().map(|o| &o.kind), Some(Obj::Pair { car, .. }) if self.keyword(car.get()) == "define")
    }

    /// `top` is true for a top-level form, or a `begin` at top level, where
    /// `define` defines a global.
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
                match self.keyword(car.get()) {
                    "quote" => self.quote(&items),
                    "if" => self.if_(&items),
                    "define" if top => self.define(&items),
                    "define" => Err(format!("define: only allowed at top level or at the start of a body: {}", print_value(x))),
                    "set!" => self.set(&items),
                    "begin" => self.begin(&items, top),
                    "lambda" if items.len() >= 3 => self.lambda("", items[1], &items[2..]),
                    "lambda" => Err(syntax_error("lambda", &items)),
                    "let" => self.let_(&items),
                    "letrec" | "letrec*" => self.letrec(&items),
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

    /// A top-level `(define name expr)` or `(define (name . formals) body
    /// ...)`.
    fn define(&mut self, items: &[Value]) -> Result<Expr, String> {
        let (name, def) = definition(items)?;
        let value = self.definition_value(name, def)?;
        Ok(Expr::GlobalDefine(self.env.cell(self.heap, name), Box::new(value)))
    }

    /// Expand the value of a definition of `name`. A procedure gets `name`
    /// as its name.
    fn definition_value(&mut self, name: GcRef, def: Definition) -> Result<Expr, String> {
        let name_text = print_value(Value::obj(name));
        match def {
            Definition::Procedure(formals, body) => self.lambda(&name_text, formals, &body),
            Definition::Expr(x) => {
                let mut value = self.expr(x, false)?;
                if let Expr::Lambda(l) = &mut value
                    && l.name.is_empty()
                {
                    l.name = name_text.into();
                }
                Ok(value)
            }
        }
    }

    /// `(set! name expr)`.
    fn set(&mut self, items: &[Value]) -> Result<Expr, String> {
        let [_, name, value] = items else { return Err(syntax_error("set!", items)) };
        let Some(name) = symbol(*name) else { return Err(syntax_error("set!", items)) };
        let value = Box::new(self.expr(*value, false)?);
        Ok(match self.local(name) {
            Some(id) => Expr::LocalSet(id, value),
            None => Expr::GlobalSet(self.env.cell(self.heap, name), value),
        })
    }

    fn begin(&mut self, items: &[Value], top: bool) -> Result<Expr, String> {
        if items.len() == 1 && top {
            // `(begin)` is only legal at top level; R7RS leaves its value
            // unspecified.
            return Ok(Expr::Const(Value::UNSPECIFIED));
        }
        self.sequence(&items[1..], top).map_err(|_| "begin: empty in an expression".into())
    }

    /// A sequence of expressions: one expression, or a `Seq`.
    fn sequence(&mut self, forms: &[Value], top: bool) -> Result<Expr, String> {
        let mut body = forms.iter().map(|&x| self.expr(x, top)).collect::<Result<Vec<_>, _>>()?;
        match body.len() {
            0 => Err("empty body".into()),
            1 => Ok(body.pop().unwrap()),
            _ => Ok(Expr::Seq(body)),
        }
    }

    /// The body of a `lambda`, `let` or `letrec`: definitions, then at least
    /// one expression. The definitions become a `Letrec` around the
    /// expressions (R7RS §5.3.2: internal definitions are `letrec*`).
    fn body(&mut self, forms: &[Value]) -> Result<Expr, String> {
        let n = forms.iter().take_while(|&&f| self.is_definition(f)).count();
        let (defs, exprs) = forms.split_at(n);
        if defs.is_empty() {
            return self.sequence(exprs, false);
        }
        if exprs.is_empty() {
            return Err("body: no expression after the definitions".into());
        }
        let mut parsed: Vec<(GcRef, Definition)> = Vec::new();
        for &d in defs {
            let (name, def) = definition(&list_to_vec(d).unwrap())?;
            if parsed.iter().any(|(n, _)| *n == name) {
                return Err(format!("define: duplicate definition: {}", print_value(Value::obj(name))));
            }
            parsed.push((name, def));
        }
        let vars: Vec<Var> = parsed.iter().map(|(name, _)| self.fresh(*name)).collect();
        self.scoped(&vars, |e| {
            let mut bindings = Vec::new();
            for ((name, def), var) in parsed.into_iter().zip(&vars) {
                bindings.push((*var, e.definition_value(name, def)?));
            }
            Ok(Expr::Letrec(bindings, Box::new(e.sequence(exprs, false)?)))
        })
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
        let body = self.scoped(&all, |e| e.body(body))?;
        Ok(Expr::Lambda(Box::new(Lambda { name: name.into(), params, rest, body })))
    }

    /// `(let ((v init) ...) body ...)`, or a named let.
    fn let_(&mut self, items: &[Value]) -> Result<Expr, String> {
        if items.len() < 3 {
            return Err(syntax_error("let", items));
        }
        if let Some(name) = symbol(items[1]) {
            return self.named_let(name, items);
        }
        let specs = self.bindings(items[1], "let")?;
        let mut bindings = Vec::new();
        for (name, init) in specs {
            // The inits are expanded outside the new scope.
            let init = self.expr(init, false)?;
            bindings.push((self.fresh(name), init));
        }
        let vars: Vec<Var> = bindings.iter().map(|(v, _)| *v).collect();
        let body = self.scoped(&vars, |e| e.body(&items[2..]))?;
        Ok(Expr::Let(bindings, Box::new(body)))
    }

    /// `(let name ((v init) ...) body ...)` is
    /// `((letrec ((name (lambda (v ...) body ...))) name) init ...)`.
    fn named_let(&mut self, name: GcRef, items: &[Value]) -> Result<Expr, String> {
        if items.len() < 4 {
            return Err(syntax_error("let", items));
        }
        let specs = self.bindings(items[2], "let")?;
        let mut inits = Vec::new();
        for &(_, init) in &specs {
            inits.push(self.expr(init, false)?);
        }
        let formals = self.heap.list(specs.iter().map(|&(v, _)| Value::obj(v)).collect());
        let var = self.fresh(name);
        let proc = self.scoped(&[var], |e| {
            let lambda = e.lambda(&print_value(Value::obj(name)), formals, &items[3..])?;
            Ok(Expr::Letrec(vec![(var, lambda)], Box::new(Expr::LocalRef(var.id))))
        })?;
        Ok(Expr::Call(Box::new(proc), inits))
    }

    /// `(letrec* ((v init) ...) body ...)`, and `letrec`, which is
    /// allowed to be the same: R7RS only says what `letrec` does when no
    /// init refers to another variable's value, and then the two agree.
    fn letrec(&mut self, items: &[Value]) -> Result<Expr, String> {
        let who = self.keyword(items[0]);
        if items.len() < 3 {
            return Err(syntax_error(who, items));
        }
        let specs = self.bindings(items[1], who)?;
        let vars: Vec<Var> = specs.iter().map(|&(name, _)| self.fresh(name)).collect();
        self.scoped(&vars, |e| {
            let mut bindings = Vec::new();
            for (&(name, init), var) in specs.iter().zip(&vars) {
                bindings.push((*var, e.definition_value(name, Definition::Expr(init))?));
            }
            Ok(Expr::Letrec(bindings, Box::new(e.body(&items[2..])?)))
        })
    }

    /// The `((v init) ...)` of a `let` or `letrec`: each variable and its
    /// unexpanded init.
    fn bindings(&self, specs: Value, who: &str) -> Result<Vec<(GcRef, Value)>, String> {
        let specs = list_to_vec(specs).ok_or_else(|| format!("{}: bad bindings: {}", who, print_value(specs)))?;
        let mut out: Vec<(GcRef, Value)> = Vec::new();
        for spec in specs {
            match list_to_vec(spec).as_deref() {
                Some(&[name, init]) if symbol(name).is_some() => {
                    let name = symbol(name).unwrap();
                    if out.iter().any(|&(n, _)| n == name) {
                        return Err(format!("{}: duplicate variable: {}", who, print_value(Value::obj(name))));
                    }
                    out.push((name, init));
                }
                _ => return Err(format!("{}: bad binding: {}", who, print_value(spec))),
            }
        }
        Ok(out)
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

/// Split a `define` form into the name and the unexpanded value.
fn definition(items: &[Value]) -> Result<(GcRef, Definition), String> {
    if let [_, target, body @ ..] = items
        && let Some(Obj::Pair { car, cdr }) = target.as_obj().map(|o| &o.kind)
        && let Some(name) = symbol(car.get())
        && !body.is_empty()
    {
        return Ok((name, Definition::Procedure(cdr.get(), body.to_vec())));
    }
    match items {
        [_, name, value] if symbol(*name).is_some() => Ok((symbol(*name).unwrap(), Definition::Expr(*value))),
        _ => Err(syntax_error("define", items)),
    }
}

fn syntax_error(keyword: &str, items: &[Value]) -> String {
    format!("{}: bad syntax: ({})", keyword, items.iter().map(|&v| print_value(v)).collect::<Vec<_>>().join(" "))
}
