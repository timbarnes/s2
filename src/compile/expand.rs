//! The expander: datum to core AST, with hygienic macros (D10, plan/03 §12).
//!
//! **The syntactic environment** says what each identifier means: a local
//! variable, a macro, a core form, or a global variable. Local scopes are a
//! stack of frames (`frames`), each a list of (identifier, `Binding`); below
//! them is the top-level environment (`globals::Env`), which binds the core
//! forms, top-level macros and global variables.
//!
//! **Core forms** are the cases in `core` below. Everything else that R7RS
//! calls syntax (`cond`, `case`, `and`, `do`, …) is a `syntax-rules` macro
//! in `scheme/core.scm`.
//!
//! **Hygiene by renaming** (from s1, `design/hygiene-design.md`): expanding
//! a macro use renames each identifier its template introduces to an alias,
//! which remembers the identifier it renames and how many frames were
//! visible where the macro was defined. `resolve` looks an alias up as
//! itself first, which finds only what the expansion itself bound; failing
//! that, it looks up the original in the macro's definition scope. So a
//! macro's temporaries can't capture the user's variables, and the user's
//! bindings can't capture the macro's free identifiers. `quote` turns
//! aliases back into symbols.
//!
//! Expansion runs no Scheme code: `syntax-rules` is plain Rust
//! (`syntax_rules.rs`). Nothing here can reach a GC safepoint, so the
//! values it holds need no rooting (plan/03 §4).

use std::collections::VecDeque;

use crate::compile::ast::{Expr, Lambda, Var, VarId};
use crate::compile::syntax_rules::{self, identifier, list_parts, strip, strip_datum};
use crate::globals::{Env, TopBinding};
use crate::heap::{GcRef, Heap, Obj, list_to_vec};
use crate::printer::print_value;
use crate::value::Value;

/// The forms the expander implements itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Core {
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    Lambda,
    Define,
    Set,
    If,
    Begin,
    Let,
    Letrec,
    LetrecStar,
    DefineSyntax,
    LetSyntax,
    LetrecSyntax,
    SyntaxRules,
}

/// The core forms' names, bound in every top-level environment.
pub const CORE_FORMS: &[(&str, Core)] = &[
    ("quote", Core::Quote),
    ("quasiquote", Core::Quasiquote),
    ("unquote", Core::Unquote),
    ("unquote-splicing", Core::UnquoteSplicing),
    ("lambda", Core::Lambda),
    ("define", Core::Define),
    ("set!", Core::Set),
    ("if", Core::If),
    ("begin", Core::Begin),
    ("let", Core::Let),
    ("letrec", Core::Letrec),
    ("letrec*", Core::LetrecStar),
    ("define-syntax", Core::DefineSyntax),
    ("let-syntax", Core::LetSyntax),
    ("letrec-syntax", Core::LetrecSyntax),
    ("syntax-rules", Core::SyntaxRules),
];

/// What an identifier means at a point in the code. Two identifiers are
/// `free-identifier=?` when they resolve to equal bindings.
#[derive(Clone, Copy, PartialEq)]
enum Binding {
    Local(VarId),
    Macro(GcRef),
    Core(Core),
    /// A global variable, bound or not, keyed by its identifier in the
    /// top-level environment.
    Global(GcRef),
}

/// The deepest nesting of macro uses allowed, to turn a macro that
/// expands forever into an error instead of a stack overflow. Expansion
/// and code generation are recursive, so `main` runs them on a large stack
/// (and `.cargo/config.toml` does the same for tests).
const MAX_NESTED_EXPANSIONS: usize = 10_000;

/// Expand a top-level form.
pub fn expand(heap: &mut Heap, env: &mut Env, x: Value) -> Result<Expr, String> {
    Expander { heap, env, frames: Vec::new(), next_id: 0, nesting: 0 }.expr(x, true)
}

struct Expander<'a> {
    heap: &'a mut Heap,
    env: &'a mut Env,
    /// The local scopes, innermost last.
    frames: Vec<Vec<(GcRef, Binding)>>,
    next_id: VarId,
    /// Macro expansions in progress, each inside the last.
    nesting: usize,
}

/// The value of a `define`, before expansion: `(define (name . formals)
/// body ...)` or `(define name expr)`.
enum Definition {
    Procedure(Value, Vec<Value>),
    Expr(Value),
}

fn kind<'a>(v: Value) -> Option<&'a Obj> {
    v.as_obj().map(|o| &o.kind)
}

/// An identifier's name, for messages.
fn show(id: GcRef) -> String {
    print_value(Value::obj(id))
}

impl Expander<'_> {
    /// What `id` means here.
    fn resolve(&self, id: GcRef) -> Binding {
        self.resolve_in(id, self.frames.len())
    }

    /// What `id` means in the scope made of the first `depth` frames and
    /// the top level. An alias bound nowhere means what its original means
    /// in the macro's definition scope.
    fn resolve_in(&self, mut id: GcRef, mut depth: usize) -> Binding {
        loop {
            for frame in self.frames[..depth].iter().rev() {
                if let Some(&(_, b)) = frame.iter().rev().find(|(k, _)| *k == id) {
                    return b;
                }
            }
            match self.env.lookup(id) {
                Some(TopBinding::Variable(_)) => return Binding::Global(id),
                Some(TopBinding::Macro(m)) => return Binding::Macro(m),
                Some(TopBinding::Core(c)) => return Binding::Core(c),
                None => {}
            }
            match &id.get().kind {
                Obj::Alias { original, depth: d } => {
                    debug_assert!(*d as usize <= depth, "an alias used outside its macro's scope");
                    id = original.as_ref().unwrap();
                    depth = *d as usize;
                }
                _ => return Binding::Global(id),
            }
        }
    }

    /// The core form `x` is a use of, if it is one.
    fn head_core(&self, x: Value) -> Option<Core> {
        match kind(x) {
            Some(Obj::Pair { car, .. }) => match identifier(car.get()).map(|h| self.resolve(h)) {
                Some(Binding::Core(c)) => Some(c),
                _ => None,
            },
            _ => None,
        }
    }

    /// The macro `x` is a use of, if it is one.
    fn head_macro(&self, x: Value) -> Option<GcRef> {
        match kind(x) {
            Some(Obj::Pair { car, .. }) => match identifier(car.get()).map(|h| self.resolve(h)) {
                Some(Binding::Macro(m)) => Some(m),
                _ => None,
            },
            _ => None,
        }
    }

    /// Expand one use of the macro `m`.
    fn expand_macro(&mut self, m: GcRef, x: Value) -> Result<Value, String> {
        let Obj::Macro(rules) = &m.get().kind else { unreachable!("not a macro") };
        let depth = rules.depth as usize;
        let same = |literal: GcRef, input: GcRef| self.resolve_in(literal, depth) == self.resolve(input);
        let (template, binds) = rules.match_use(x, &same)?;
        rules.instantiate(self.heap, template, &binds)
    }

    /// Expand macro uses at the head of `x` until it isn't one.
    fn expand_head(&mut self, mut x: Value) -> Result<Value, String> {
        while let Some(m) = self.head_macro(x) {
            x = self.expand_macro(m, x)?;
        }
        Ok(x)
    }

    fn fresh(&mut self, id: GcRef) -> Var {
        self.next_id += 1;
        Var { id: self.next_id, name: Value::obj(id) }
    }

    /// Run `f` in a new frame binding `vars`.
    fn scoped(&mut self, vars: &[Var], f: impl FnOnce(&mut Self) -> Result<Expr, String>) -> Result<Expr, String> {
        self.frames.push(vars.iter().map(|v| (v.name.as_ref().unwrap(), Binding::Local(v.id))).collect());
        let result = f(self);
        self.frames.pop();
        result
    }

    fn items(&self, x: Value) -> Result<Vec<Value>, String> {
        list_to_vec(x).ok_or_else(|| format!("improper list in code: {}", print_value(x)))
    }

    /// `top` is true for a top-level form, or a `begin` at top level, where
    /// `define` and `define-syntax` define globally.
    fn expr(&mut self, x: Value, top: bool) -> Result<Expr, String> {
        if let Some(id) = identifier(x) {
            return self.variable(id);
        }
        if let Some(c) = self.head_core(x) {
            return self.core(c, x, top);
        }
        if let Some(m) = self.head_macro(x) {
            if self.nesting == MAX_NESTED_EXPANSIONS {
                return Err(format!("macro uses nested more than {} deep: {}", MAX_NESTED_EXPANSIONS, print_value(x)));
            }
            let expansion = self.expand_macro(m, x)?;
            self.nesting += 1;
            let result = self.expr(expansion, top);
            self.nesting -= 1;
            return result;
        }
        match kind(x) {
            Some(Obj::Pair { .. }) => self.call(x),
            _ if x.is_nil() => Err("empty combination: ()".into()),
            // Numbers, strings, vectors and so on evaluate to themselves.
            _ => Ok(Expr::Const(strip_datum(self.heap, x))),
        }
    }

    fn variable(&mut self, id: GcRef) -> Result<Expr, String> {
        match self.resolve(id) {
            Binding::Local(v) => Ok(Expr::LocalRef(v)),
            Binding::Global(key) => Ok(Expr::GlobalRef(self.env.cell(self.heap, key))),
            Binding::Macro(_) | Binding::Core(_) => Err(format!("{}: syntax used as a variable", show(id))),
        }
    }

    fn core(&mut self, c: Core, x: Value, top: bool) -> Result<Expr, String> {
        let items = self.items(x)?;
        let keyword = syntax_rules::name(identifier(items[0]).unwrap());
        match c {
            Core::Quote => match items[..] {
                [_, datum] => Ok(Expr::Const(strip_datum(self.heap, datum))),
                _ => Err(syntax_error(keyword, &items)),
            },
            Core::Quasiquote => match items[..] {
                [_, template] => self.quasi(template, 1),
                _ => Err(syntax_error(keyword, &items)),
            },
            Core::Unquote | Core::UnquoteSplicing => Err(format!("{}: not in a quasiquote: {}", keyword, print_value(x))),
            Core::If => self.if_(&items),
            Core::Define | Core::DefineSyntax if !top => {
                Err(format!("{}: only allowed at top level or at the start of a body: {}", keyword, print_value(x)))
            }
            Core::Define => self.define(&items),
            Core::DefineSyntax => self.define_syntax(&items),
            Core::Set => self.set(&items),
            Core::Begin => self.begin(&items, top),
            Core::Lambda if items.len() >= 3 => self.lambda("", items[1], &items[2..]),
            Core::Lambda => Err(syntax_error(keyword, &items)),
            Core::Let => self.let_(&items),
            Core::Letrec | Core::LetrecStar => self.letrec(keyword, &items),
            Core::LetSyntax => self.let_syntax(keyword, &items, false),
            Core::LetrecSyntax => self.let_syntax(keyword, &items, true),
            Core::SyntaxRules => Err(format!("syntax-rules: only allowed as a macro's transformer: {}", print_value(x))),
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
        // The cell first: if `name` is an alias, only its own cell makes
        // recursive references inside the value find it.
        let cell = self.env.cell(self.heap, name);
        let value = self.definition_value(name, def)?;
        Ok(Expr::GlobalDefine(cell, Box::new(value)))
    }

    /// A top-level `(define-syntax name transformer)`. Its value, like
    /// `define`'s, is the name.
    fn define_syntax(&mut self, items: &[Value]) -> Result<Expr, String> {
        let [_, name, spec] = items else { return Err(syntax_error("define-syntax", items)) };
        let Some(name) = identifier(*name) else { return Err(syntax_error("define-syntax", items)) };
        let m = self.transformer(spec, 0)?;
        self.env.define_syntax(name, m);
        Ok(Expr::Const(Value::obj(strip(name))))
    }

    /// Parse `spec`, which must be a `syntax-rules` form, into a macro
    /// defined in the scope of the first `depth` frames.
    fn transformer(&mut self, spec: &Value, depth: usize) -> Result<GcRef, String> {
        if self.head_core(*spec) != Some(Core::SyntaxRules) {
            return Err(format!("not a syntax-rules transformer: {}", print_value(*spec)));
        }
        let rules = syntax_rules::parse(*spec, depth as u32)?;
        Ok(self.heap.make_macro(rules))
    }

    /// Expand the value of a definition of `name`. A procedure gets `name`
    /// as its name.
    fn definition_value(&mut self, name: GcRef, def: Definition) -> Result<Expr, String> {
        let name_text = show(name);
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
        let Some(name) = identifier(*name) else { return Err(syntax_error("set!", items)) };
        let value = Box::new(self.expr(*value, false)?);
        match self.resolve(name) {
            Binding::Local(id) => Ok(Expr::LocalSet(id, value)),
            Binding::Global(key) => Ok(Expr::GlobalSet(self.env.cell(self.heap, key), value)),
            Binding::Macro(_) | Binding::Core(_) => Err(format!("set!: can't assign to syntax: {}", show(name))),
        }
    }

    fn begin(&mut self, items: &[Value], top: bool) -> Result<Expr, String> {
        if items.len() == 1 && top {
            // `(begin)` is only legal at top level; R7RS leaves its value
            // unspecified.
            return Ok(Expr::Const(Value::UNSPECIFIED));
        }
        if top {
            // Make the cells of the begin's definitions first, so a form can
            // refer to one defined later in the same begin. Only a hidden
            // definition (keyed by a macro's alias) needs this: until its
            // cell exists, the alias would resolve to the plain global.
            for &form in &items[1..] {
                if self.head_core(form) == Some(Core::Define)
                    && let Ok((name, _)) = definition(&self.items(form)?)
                {
                    self.env.cell(self.heap, name);
                }
            }
        }
        self.sequence(&items[1..], top).map_err(|e| if items.len() == 1 { "begin: empty in an expression".into() } else { e })
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

    /// A body: of a `lambda`, `let`, `letrec`, `let-syntax` or
    /// `letrec-syntax` (R7RS §5.3.2, §4.3.1). Its definitions get a new
    /// frame, and become a `Letrec` around the expressions.
    fn body(&mut self, forms: &[Value]) -> Result<Expr, String> {
        self.frames.push(Vec::new());
        let result = self.body_in_frame(forms);
        self.frames.pop();
        result
    }

    /// Two passes. The first finds the definitions: it expands each form's
    /// head until it knows whether the form is a definition (a macro use
    /// may expand into one), splices `begin`s, binds each defined name in
    /// the frame, and stops at the first expression. The second expands
    /// the definitions' values and the expressions, with every definition
    /// in scope, so a macro defined in the body can refer to a procedure
    /// defined after it.
    fn body_in_frame(&mut self, forms: &[Value]) -> Result<Expr, String> {
        let mut pending: VecDeque<Value> = forms.iter().copied().collect();
        let mut defs: Vec<(Var, Definition)> = Vec::new();
        let mut exprs = Vec::new();
        while let Some(form) = pending.pop_front() {
            let form = self.expand_head(form)?;
            match self.head_core(form) {
                Some(Core::Begin) => {
                    let items = self.items(form)?;
                    items[1..].iter().rev().for_each(|&f| pending.push_front(f));
                }
                Some(Core::Define) => {
                    let (name, def) = definition(&self.items(form)?)?;
                    self.check_unbound_in_frame(name)?;
                    let var = self.fresh(name);
                    self.frames.last_mut().unwrap().push((name, Binding::Local(var.id)));
                    defs.push((var, def));
                }
                Some(Core::DefineSyntax) => {
                    let items = self.items(form)?;
                    let [_, name, spec] = items[..] else { return Err(syntax_error("define-syntax", &items)) };
                    let Some(name) = identifier(name) else { return Err(syntax_error("define-syntax", &items)) };
                    self.check_unbound_in_frame(name)?;
                    // The body's whole frame is the macro's scope.
                    let m = self.transformer(&spec, self.frames.len())?;
                    self.frames.last_mut().unwrap().push((name, Binding::Macro(m)));
                }
                _ => {
                    exprs.push(form);
                    exprs.extend(pending.drain(..));
                }
            }
        }
        if exprs.is_empty() {
            return Err("body: no expression after the definitions".into());
        }
        let mut bindings = Vec::new();
        for (var, def) in defs {
            bindings.push((var, self.definition_value(var.name.as_ref().unwrap(), def)?));
        }
        let body = self.sequence(&exprs, false)?;
        Ok(if bindings.is_empty() { body } else { Expr::Letrec(bindings, Box::new(body)) })
    }

    fn check_unbound_in_frame(&self, name: GcRef) -> Result<(), String> {
        if self.frames.last().unwrap().iter().any(|&(k, _)| k == name) {
            return Err(format!("define: duplicate definition: {}", show(name)));
        }
        Ok(())
    }

    /// `(lambda formals body ...)`. `formals` is a list of identifiers,
    /// possibly improper (`(a b . rest)`), or a single identifier for all
    /// the arguments.
    fn lambda(&mut self, name: &str, formals: Value, body: &[Value]) -> Result<Expr, String> {
        let (items, tail) = list_parts(formals);
        let mut params = Vec::new();
        for item in items {
            let id = identifier(item).ok_or_else(|| format!("lambda: bad parameter: {}", print_value(item)))?;
            params.push(self.fresh(id));
        }
        let rest = match identifier(tail) {
            Some(id) => Some(self.fresh(id)),
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
        if let Some(name) = identifier(items[1]) {
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
            let lambda = e.lambda(&show(name), formals, &items[3..])?;
            Ok(Expr::Letrec(vec![(var, lambda)], Box::new(Expr::LocalRef(var.id))))
        })?;
        Ok(Expr::Call(Box::new(proc), inits))
    }

    /// `(letrec* ((v init) ...) body ...)`, and `letrec`, which is
    /// allowed to be the same: R7RS only says what `letrec` does when no
    /// init refers to another variable's value, and then the two agree.
    fn letrec(&mut self, keyword: &str, items: &[Value]) -> Result<Expr, String> {
        if items.len() < 3 {
            return Err(syntax_error(keyword, items));
        }
        let specs = self.bindings(items[1], keyword)?;
        let vars: Vec<Var> = specs.iter().map(|&(name, _)| self.fresh(name)).collect();
        self.scoped(&vars, |e| {
            let mut bindings = Vec::new();
            for (&(name, init), var) in specs.iter().zip(&vars) {
                bindings.push((*var, e.definition_value(name, Definition::Expr(init))?));
            }
            Ok(Expr::Letrec(bindings, Box::new(e.body(&items[2..])?)))
        })
    }

    /// `(let-syntax ((name transformer) ...) body ...)`, or `letrec-syntax`
    /// (`rec`), whose transformers are defined inside the new frame, so
    /// they can refer to each other. The body is a body, so definitions in
    /// it are local (as in s1).
    fn let_syntax(&mut self, keyword: &str, items: &[Value], rec: bool) -> Result<Expr, String> {
        if items.len() < 3 {
            return Err(syntax_error(keyword, items));
        }
        let specs = self.bindings(items[1], keyword)?;
        let depth = self.frames.len() + rec as usize;
        self.frames.push(Vec::new());
        let result = (|| {
            for (name, spec) in specs {
                let m = self.transformer(&spec, depth)?;
                self.frames.last_mut().unwrap().push((name, Binding::Macro(m)));
            }
            self.body(&items[2..])
        })();
        self.frames.pop();
        result
    }

    /// The `((v init) ...)` of a `let`, `letrec` or `let-syntax`: each
    /// identifier and its unexpanded init.
    fn bindings(&self, specs: Value, who: &str) -> Result<Vec<(GcRef, Value)>, String> {
        let specs = list_to_vec(specs).ok_or_else(|| format!("{}: bad bindings: {}", who, print_value(specs)))?;
        let mut out: Vec<(GcRef, Value)> = Vec::new();
        for spec in specs {
            match list_to_vec(spec).as_deref() {
                Some(&[name, init]) if identifier(name).is_some() => {
                    let name = identifier(name).unwrap();
                    if out.iter().any(|&(n, _)| n == name) {
                        return Err(format!("{}: duplicate variable: {}", who, show(name)));
                    }
                    out.push((name, init));
                }
                _ => return Err(format!("{}: bad binding: {}", who, print_value(spec))),
            }
        }
        Ok(out)
    }

    fn call(&mut self, x: Value) -> Result<Expr, String> {
        let items = self.items(x)?;
        let f = self.expr(items[0], false)?;
        let args = items[1..].iter().map(|&x| self.expr(x, false)).collect::<Result<Vec<_>, _>>()?;
        Ok(Expr::Call(Box::new(f), args))
    }

    // -----------------------------------------------------------------------
    // Quasiquote
    // -----------------------------------------------------------------------

    /// The quasiquoted template `x` at nesting `depth`, as calls to
    /// `cons`, `append` and `list->vector` (R7RS §4.2.8). Parts with no
    /// unquote in them are constants.
    // ponytail: the calls go through the global cells of `cons`, `append`
    // and `list->vector`, so a program that redefines those globally
    // changes quasiquote. Private primitives would fix that if it matters.
    fn quasi(&mut self, x: Value, depth: usize) -> Result<Expr, String> {
        if !self.has_unquote(x) {
            return Ok(Expr::Const(strip_datum(self.heap, x)));
        }
        if let Some((c, arg)) = self.unquote_form(x) {
            return match c {
                Core::Unquote if depth == 1 => self.expr(arg, false),
                Core::Unquote => self.tagged("unquote", arg, depth - 1),
                Core::UnquoteSplicing if depth == 1 => Err(format!("unquote-splicing: not in a list: {}", print_value(x))),
                Core::UnquoteSplicing => self.tagged("unquote-splicing", arg, depth - 1),
                _ => self.tagged("quasiquote", arg, depth + 1),
            };
        }
        match kind(x) {
            Some(Obj::Pair { car, cdr }) => {
                let (car, cdr) = (car.get(), cdr.get());
                if depth == 1
                    && let Some((Core::UnquoteSplicing, arg)) = self.unquote_form(car)
                {
                    let spliced = self.expr(arg, false)?;
                    let rest = self.quasi(cdr, depth)?;
                    return Ok(self.global_call("append", vec![spliced, rest]));
                }
                let a = self.quasi(car, depth)?;
                let d = self.quasi(cdr, depth)?;
                Ok(self.global_call("cons", vec![a, d]))
            }
            Some(Obj::Vector(items)) => {
                let list = self.heap.list(items.iter().map(|c| c.get()).collect());
                let l = self.quasi(list, depth)?;
                Ok(self.global_call("list->vector", vec![l]))
            }
            _ => unreachable!("has_unquote is false for atoms"),
        }
    }

    /// `(tag arg)` as data, with `arg` quasiquoted at `depth`.
    fn tagged(&mut self, tag: &str, arg: Value, depth: usize) -> Result<Expr, String> {
        let tag = Expr::Const(self.heap.intern(tag));
        let inner = self.quasi(arg, depth)?;
        let tail = self.global_call("cons", vec![inner, Expr::Const(Value::NIL)]);
        Ok(self.global_call("cons", vec![tag, tail]))
    }

    /// `x` as `(unquote e)`, `(unquote-splicing e)` or `(quasiquote e)`.
    fn unquote_form(&self, x: Value) -> Option<(Core, Value)> {
        let (items, tail) = list_parts(x);
        match (items.as_slice(), tail.is_nil()) {
            ([head, arg], true) => match identifier(*head).map(|h| self.resolve(h)) {
                Some(Binding::Core(c @ (Core::Unquote | Core::UnquoteSplicing | Core::Quasiquote))) => Some((c, *arg)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether `x` contains an unquote at any depth.
    // ponytail: recursive, so very long quasiquoted lists use Rust stack,
    // and it loops on cyclic templates (only possible via datum labels).
    fn has_unquote(&self, x: Value) -> bool {
        match kind(x) {
            Some(Obj::Pair { car, cdr }) => {
                matches!(self.unquote_form(x), Some((Core::Unquote | Core::UnquoteSplicing, _)))
                    || self.has_unquote(car.get())
                    || self.has_unquote(cdr.get())
            }
            Some(Obj::Vector(items)) => items.iter().any(|c| self.has_unquote(c.get())),
            _ => false,
        }
    }

    fn global_call(&mut self, name: &str, args: Vec<Expr>) -> Expr {
        let sym = self.heap.intern(name).as_ref().unwrap();
        Expr::Call(Box::new(Expr::GlobalRef(self.env.cell(self.heap, sym))), args)
    }
}

/// Split a `define` form into the name and the unexpanded value.
fn definition(items: &[Value]) -> Result<(GcRef, Definition), String> {
    if let [_, target, body @ ..] = items
        && let Some(Obj::Pair { car, cdr }) = kind(*target)
        && let Some(name) = identifier(car.get())
        && !body.is_empty()
    {
        return Ok((name, Definition::Procedure(cdr.get(), body.to_vec())));
    }
    match items {
        [_, name, value] if identifier(*name).is_some() => Ok((identifier(*name).unwrap(), Definition::Expr(*value))),
        _ => Err(syntax_error("define", items)),
    }
}

fn syntax_error(keyword: &str, items: &[Value]) -> String {
    format!("{}: bad syntax: ({})", keyword, items.iter().map(|&v| print_value(v)).collect::<Vec<_>>().join(" "))
}
