//! What the front end in Scheme (`scheme/compiler/`) uses of the runtime
//! (plan/08, C3): aliases, top-level environments and their cells, macros,
//! the library registry, and reading the files `include` names. These stay
//! Rust objects, shared with the VM, compiled files and the Rust front end.
//!
//! A binding, as data: `(variable . cell)`, `(macro . m)` or `(core .
//! name)`, where `name` is the core form's symbol.
//!
//! Errors carry the Rust front end's messages exactly, so the two front
//! ends report the same.

use std::path::Path;

use crate::builtins::{Error, Primitive, err, expected, kind, list_arg, prim};
use crate::globals::CORE_FORMS;
use crate::compile::syntax_rules::{SyntaxRules, identifier, strip_datum};
use crate::globals::{Env, TopBinding, cell_value, env_of};
use crate::heap::{GcRef, Heap, Obj};
use crate::libraries::{LibraryName, find_library_file, read_file_forms};
use crate::value::Value;

fn env_arg(v: Value, who: &str) -> Result<GcRef, Error> {
    match kind(v) {
        Some(Obj::Environment(_)) => Ok(v.as_ref().unwrap()),
        _ => Err(expected(who, "an environment", v)),
    }
}

fn id_arg(v: Value, who: &str) -> Result<GcRef, Error> {
    identifier(v).ok_or_else(|| expected(who, "an identifier", v))
}

fn cell_arg(v: Value, who: &str) -> Result<GcRef, Error> {
    match kind(v) {
        Some(Obj::Global { .. }) => Ok(v.as_ref().unwrap()),
        _ => Err(expected(who, "a global variable's cell", v)),
    }
}

fn binding_datum(heap: &mut Heap, b: TopBinding) -> Value {
    let (tag, x) = match b {
        TopBinding::Variable(c) => ("variable", Value::obj(c)),
        TopBinding::Macro(m) => ("macro", Value::obj(m)),
        TopBinding::Core(c) => ("core", heap.intern(CORE_FORMS.iter().find(|&&(_, f)| f == c).unwrap().0)),
    };
    let tag = heap.intern(tag);
    heap.cons(tag, x)
}

fn datum_binding(d: Value) -> Result<TopBinding, Error> {
    let bad = || err!("not a binding: {}", crate::printer::print_value(d));
    let Some(Obj::Pair { car, cdr }) = kind(d) else { return Err(bad()) };
    let tag = car.get().as_ref().filter(|s| matches!(s.get().kind, Obj::Symbol(_))).map(crate::compile::syntax_rules::name).ok_or_else(bad)?;
    let x = cdr.get();
    Ok(match (tag, kind(x)) {
        ("variable", Some(Obj::Global { .. })) => TopBinding::Variable(x.as_ref().unwrap()),
        ("macro", Some(Obj::Macro(_))) => TopBinding::Macro(x.as_ref().unwrap()),
        ("core", Some(Obj::Symbol(name))) => TopBinding::Core(CORE_FORMS.iter().find(|&&(n, _)| n == &**name).ok_or_else(bad)?.1),
        _ => return Err(bad()),
    })
}

/// A library name given as a list of strings, as `library_name` makes it.
fn library_name_arg(v: Value, who: &str) -> Result<LibraryName, Error> {
    list_arg(v, who)?
        .into_iter()
        .map(|p| match kind(p) {
            Some(Obj::Str(s)) => Ok(s.borrow().to_string()),
            _ => Err(expected(who, "a string", p)),
        })
        .collect()
}

fn string_value(v: Value, who: &str) -> Result<String, Error> {
    match kind(v) {
        Some(Obj::Str(s)) => Ok(s.borrow().to_string()),
        _ => Err(expected(who, "a string", v)),
    }
}

pub static PRIMITIVES: &[Primitive] = &[
    // Aliases: the renamed identifiers of hygiene (plan/03 §12).
    prim("%alias?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Alias { .. }))))),
    prim("%make-alias", 3, Some(3), |h, a| {
        id_arg(a[0], "%make-alias")?;
        let depth = a[1].as_fixnum().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| expected("%make-alias", "a depth", a[1]))?;
        Ok(h.alias(a[0], depth, env_arg(a[2], "%make-alias")?))
    }),
    // #(original depth env)
    prim("%alias-parts", 1, Some(1), |h, a| match kind(a[0]) {
        Some(Obj::Alias { original, depth, env }) => Ok(h.vector(vec![*original, Value::fixnum(*depth as i64).unwrap(), Value::obj(*env)])),
        _ => Err(expected("%alias-parts", "an alias", a[0])),
    }),
    // A datum with its aliases replaced by their symbols.
    prim("%strip-datum", 1, Some(1), |h, a| Ok(strip_datum(h, a[0]))),
    // Environments and their bindings (globals.rs).
    prim("%make-environment", 0, Some(0), |h, _| Ok(Value::obj(h.environment(Env::empty(true))))),
    prim("%system-environment", 0, Some(0), |h, _| Ok(h.system.map(Value::obj).unwrap_or(Value::FALSE))),
    prim("%env-mutable?", 1, Some(1), |_, a| Ok(Value::bool(env_of(env_arg(a[0], "%env-mutable?")?).borrow().mutable))),
    prim("%env-lookup", 2, Some(2), |h, a| {
        let binding = env_of(env_arg(a[0], "%env-lookup")?).borrow().lookup(id_arg(a[1], "%env-lookup")?);
        Ok(match binding {
            Some(b) => binding_datum(h, b),
            None => Value::FALSE,
        })
    }),
    prim("%env-cell", 2, Some(2), |h, a| {
        let (env, id) = (env_arg(a[0], "%env-cell")?, id_arg(a[1], "%env-cell")?);
        Ok(Value::obj(env_of(env).borrow_mut().cell(h, id)))
    }),
    prim("%env-define-cell", 2, Some(2), |h, a| {
        let (env, id) = (env_arg(a[0], "%env-define-cell")?, id_arg(a[1], "%env-define-cell")?);
        Ok(Value::obj(env_of(env).borrow_mut().define_cell(h, id)?))
    }),
    prim("%env-set-cell", 2, Some(2), |h, a| {
        let (env, id) = (env_arg(a[0], "%env-set-cell")?, id_arg(a[1], "%env-set-cell")?);
        Ok(Value::obj(env_of(env).borrow_mut().set_cell(h, id)?))
    }),
    prim("%env-define-syntax", 3, Some(3), |_, a| {
        let (env, id) = (env_arg(a[0], "%env-define-syntax")?, id_arg(a[1], "%env-define-syntax")?);
        let Some(Obj::Macro(_)) = kind(a[2]) else { return Err(expected("%env-define-syntax", "a macro", a[2])) };
        env_of(env).borrow_mut().define_syntax(id, a[2].as_ref().unwrap())?;
        Ok(Value::UNSPECIFIED)
    }),
    prim("%env-import", 3, Some(3), |_, a| {
        let (env, id) = (env_arg(a[0], "%env-import")?, id_arg(a[1], "%env-import")?);
        env_of(env).borrow_mut().import(id, datum_binding(a[2])?);
        Ok(Value::UNSPECIFIED)
    }),
    prim("%cell-bound?", 1, Some(1), |_, a| Ok(Value::bool(!cell_value(cell_arg(a[0], "%cell-bound?")?).get().is_undefined()))),
    // Macros: a parsed syntax-rules, as data.
    // (%make-macro ellipsis-or-#f (literal ...) ((pattern . template) ...) depth env)
    prim("%make-macro", 5, Some(5), |h, a| {
        let who = "%make-macro";
        let ellipsis = if a[0].is_true() { Some(id_arg(a[0], who)?) } else { None };
        let literals = list_arg(a[1], who)?.into_iter().map(|l| id_arg(l, who)).collect::<Result<_, _>>()?;
        let rules = list_arg(a[2], who)?
            .into_iter()
            .map(|r| match kind(r) {
                Some(Obj::Pair { car, cdr }) => Ok((car.get(), cdr.get())),
                _ => Err(expected(who, "a (pattern . template) pair", r)),
            })
            .collect::<Result<_, _>>()?;
        let depth = a[3].as_fixnum().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| expected(who, "a depth", a[3]))?;
        Ok(Value::obj(h.make_macro(SyntaxRules { ellipsis, literals, rules, depth, env: env_arg(a[4], who)? })))
    }),
    // #(ellipsis-or-#f (literal ...) ((pattern . template) ...) depth env)
    prim("%macro-parts", 1, Some(1), |h, a| {
        let Some(Obj::Macro(m)) = kind(a[0]) else { return Err(expected("%macro-parts", "a macro", a[0])) };
        let ellipsis = m.ellipsis.map(Value::obj).unwrap_or(Value::FALSE);
        let literals = m.literals.iter().map(|&l| Value::obj(l)).collect();
        let literals = h.list(literals);
        let rules: Vec<Value> = m.rules.iter().map(|&(p, t)| h.cons(p, t)).collect();
        let rules = h.list(rules);
        Ok(h.vector(vec![ellipsis, literals, rules, Value::fixnum(m.depth as i64).unwrap(), Value::obj(m.env)]))
    }),
    // The library registry. A name is a list of strings.
    // (%library-exports name): ((name . binding) ...), or #f if it isn't registered.
    prim("%library-exports", 1, Some(1), |h, a| {
        let name = library_name_arg(a[0], "%library-exports")?;
        let Some(library) = h.libraries.get(&name) else { return Ok(Value::FALSE) };
        let exports = library.exports.clone();
        let pairs = exports
            .into_iter()
            .map(|(n, b)| {
                let b = binding_datum(h, b);
                h.cons(Value::obj(n), b)
            })
            .collect();
        Ok(h.list(pairs))
    }),
    // The file that could define a library, or #f.
    prim("%library-file", 1, Some(1), |h, a| {
        let name = library_name_arg(a[0], "%library-file")?;
        Ok(match find_library_file(&name) {
            Some(path) => h.string(&path.to_string_lossy()),
            None => Value::FALSE,
        })
    }),
    prim("%library-file-loaded?", 1, Some(1), |h, a| {
        let path = string_value(a[0], "%library-file-loaded?")?;
        Ok(Value::bool(h.libraries.is_loaded(Path::new(&path))))
    }),
    // (%read-forms file fold-case? who): every datum in a file, for include.
    prim("%read-forms", 3, Some(3), |h, a| {
        let (file, who) = (string_value(a[0], "%read-forms")?, string_value(a[2], "%read-forms")?);
        let forms = read_file_forms(h, &file, a[1].is_true(), &who)?;
        Ok(h.list(forms))
    }),
];
