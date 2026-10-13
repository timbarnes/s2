//! Libraries (R7RS §5.6, §6.12; plan/03 §13), forked from s1's
//! `libraries.rs`.
//!
//! A library is an export table: external names mapped to bindings (a
//! variable's cell, a macro, a core form). Importing binds names in the
//! importing environment to those same bindings, so importer and library
//! share each variable, and macros are exported like anything else.
//!
//! The standard libraries have no code of their own. Each is a list of the
//! names R7RS assigns it (appendix A), exported straight from the system
//! environment. Names s2 doesn't implement are left out and recorded, so a
//! test can list them. `(s2)` exports the rest of the system environment.
//!
//! Imports, `define-library`, `include` and `cond-expand` act when a form
//! is expanded (`compile/expand.rs`), using the functions here. A library
//! file is loaded the first time an import needs it: the expander records
//! the file in `pending_load` and fails, and its caller loads the file (in
//! Scheme, `%load-library`) and expands the form again.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::builtins::{Error, Primitive, err, expected, kind, list_arg, prim, string_arg};
use crate::compile::generate;
use crate::compile::expand::expand;
use crate::compile::syntax_rules::{identifier, name as id_name, strip};
use crate::fasl;
use crate::globals::{Env, TopBinding, cell_value, env_of};
use crate::heap::{GcRef, Heap, Obj, list_to_vec};
use crate::io::{PortKind, new_string_port_input};
use crate::printer::print_value;
use crate::reader::{ParseError, parse};
use crate::value::Value;

/// A library name, `(scheme base)`, as its parts: symbols, or exact
/// non-negative integers written in decimal.
pub type LibraryName = Vec<String>;

/// A registered library.
pub struct Library {
    /// External name and binding, in export order.
    pub exports: Vec<(GcRef, TopBinding)>,
    /// The environment a `define-library` body ran in; None for a standard
    /// library.
    pub env: Option<GcRef>,
    /// Names a standard library should export that s2 doesn't define.
    pub unimplemented: Vec<&'static str>,
}

/// The registered libraries, and the library files loaded.
#[derive(Default)]
pub struct Libraries {
    table: FxHashMap<LibraryName, Library>,
    loaded: FxHashSet<PathBuf>,
    /// A library file an import needs loaded before it can be expanded
    /// (see the module comment).
    pub pending_load: Option<PathBuf>,
    /// While a library file is compiled: every file read, for the compiled
    /// file's staleness check.
    reading: Option<Vec<PathBuf>>,
}

impl Libraries {
    pub fn get(&self, name: &LibraryName) -> Option<&Library> {
        self.table.get(name)
    }

    pub fn insert(&mut self, name: LibraryName, library: Library) {
        self.table.insert(name, library);
    }

    /// Whether the library file `path` has been loaded.
    pub fn is_loaded(&self, path: &Path) -> bool {
        self.loaded.contains(path)
    }

    /// The environments of the libraries `define-library` made.
    pub fn environments(&self) -> impl Iterator<Item = GcRef> + '_ {
        self.table.values().filter_map(|l| l.env)
    }

    /// The name of the library whose environment is `env`.
    pub fn name_of(&self, env: GcRef) -> Option<LibraryName> {
        self.table.iter().find(|(_, l)| l.env == Some(env)).map(|(n, _)| n.clone())
    }

    /// The names of all registered libraries, sorted.
    pub fn names(&self) -> Vec<LibraryName> {
        let mut names: Vec<_> = self.table.keys().cloned().collect();
        names.sort();
        names
    }

    /// What the GC must keep: each library's environment, export names and
    /// exported bindings.
    pub fn trace(&self, mut visit: impl FnMut(GcRef)) {
        for library in self.table.values() {
            library.env.iter().for_each(|&e| visit(e));
            for &(name, binding) in &library.exports {
                visit(name);
                if let TopBinding::Variable(r) | TopBinding::Macro(r) = binding {
                    visit(r);
                }
            }
        }
    }
}

/// `(scheme base)` and friends, from R7RS appendix A (s1's table).
const STANDARD_LIBRARIES: &[(&[&str], &[&str])] = &[
    (&["scheme", "base"], &[
        "*", "+", "-", "...", "/", "<", "<=", "=", "=>", ">", ">=", "_", "abs", "and", "append", "apply",
        "assoc", "assq", "assv", "begin", "binary-port?", "boolean=?", "boolean?", "bytevector",
        "bytevector-append", "bytevector-copy", "bytevector-copy!", "bytevector-length",
        "bytevector-u8-ref", "bytevector-u8-set!", "bytevector?", "caar", "cadr",
        "call-with-current-continuation", "call-with-port", "call-with-values", "call/cc", "car",
        "case", "cdar", "cddr", "cdr", "ceiling", "char->integer", "char-ready?", "char<=?", "char<?",
        "char=?", "char>=?", "char>?", "char?", "close-input-port", "close-output-port", "close-port",
        "complex?", "cond", "cond-expand", "cons", "current-error-port", "current-input-port",
        "current-output-port", "define", "define-record-type", "define-syntax", "define-values",
        "denominator", "do", "dynamic-wind", "else", "eof-object", "eof-object?", "eq?", "equal?",
        "eqv?", "error", "error-object-irritants", "error-object-message", "error-object?", "even?",
        "exact", "exact-integer-sqrt", "exact-integer?", "exact?", "expt", "features", "file-error?",
        "floor", "floor-quotient", "floor-remainder", "floor/", "flush-output-port", "for-each", "gcd",
        "get-output-bytevector", "get-output-string", "guard", "if", "import", "include", "include-ci",
        "inexact", "inexact?", "input-port-open?", "input-port?", "integer->char", "integer?", "lambda",
        "lcm", "length", "let", "let*", "let*-values", "let-syntax", "let-values", "letrec", "letrec*",
        "letrec-syntax", "list", "list->string", "list->vector", "list-copy", "list-ref", "list-set!",
        "list-tail", "list?", "make-bytevector", "make-list", "make-parameter", "make-string",
        "make-vector", "map", "max", "member", "memq", "memv", "min", "modulo", "negative?", "newline",
        "not", "null?", "number->string", "number?", "numerator", "odd?", "open-input-bytevector",
        "open-input-string", "open-output-bytevector", "open-output-string", "or", "output-port-open?",
        "output-port?", "pair?", "parameterize", "peek-char", "peek-u8", "positive?", "procedure?",
        "quasiquote", "quote", "quotient", "raise", "raise-continuable", "rational?", "rationalize",
        "read-bytevector", "read-bytevector!", "read-char", "read-error?", "read-line", "read-string",
        "read-u8", "real?", "remainder", "reverse", "round", "set!", "set-car!", "set-cdr!", "square",
        "string", "string->list", "string->number", "string->symbol", "string->utf8", "string->vector",
        "string-append", "string-copy", "string-copy!", "string-fill!", "string-for-each",
        "string-length", "string-map", "string-ref", "string-set!", "string<=?", "string<?", "string=?",
        "string>=?", "string>?", "string?", "substring", "symbol->string", "symbol=?", "symbol?",
        "syntax-error", "syntax-rules", "textual-port?", "truncate", "truncate-quotient",
        "truncate-remainder", "truncate/", "u8-ready?", "unless", "unquote", "unquote-splicing",
        "utf8->string", "values", "vector", "vector->list", "vector->string", "vector-append",
        "vector-copy", "vector-copy!", "vector-fill!", "vector-for-each", "vector-length", "vector-map",
        "vector-ref", "vector-set!", "vector?", "when", "with-exception-handler", "write-bytevector",
        "write-char", "write-string", "write-u8", "zero?",
    ]),
    (&["scheme", "case-lambda"], &["case-lambda"]),
    (&["scheme", "char"], &[
        "char-alphabetic?", "char-ci<=?", "char-ci<?", "char-ci=?", "char-ci>=?", "char-ci>?",
        "char-downcase", "char-foldcase", "char-lower-case?", "char-numeric?", "char-upcase",
        "char-upper-case?", "char-whitespace?", "digit-value", "string-ci<=?", "string-ci<?",
        "string-ci=?", "string-ci>=?", "string-ci>?", "string-downcase", "string-foldcase",
        "string-upcase",
    ]),
    (&["scheme", "complex"], &["angle", "imag-part", "magnitude", "make-polar", "make-rectangular", "real-part"]),
    (&["scheme", "cxr"], &[
        "caaar", "caadr", "cadar", "caddr", "cdaar", "cdadr", "cddar", "cdddr", "caaaar", "caaadr",
        "caadar", "caaddr", "cadaar", "cadadr", "caddar", "cadddr", "cdaaar", "cdaadr", "cdadar",
        "cdaddr", "cddaar", "cddadr", "cdddar", "cddddr",
    ]),
    (&["scheme", "eval"], &["environment", "eval"]),
    (&["scheme", "file"], &[
        "call-with-input-file", "call-with-output-file", "delete-file", "file-exists?",
        "open-binary-input-file", "open-binary-output-file", "open-input-file", "open-output-file",
        "with-input-from-file", "with-output-to-file",
    ]),
    (&["scheme", "inexact"], &[
        "acos", "asin", "atan", "cos", "exp", "finite?", "infinite?", "log", "nan?", "sin", "sqrt", "tan",
    ]),
    (&["scheme", "lazy"], &["delay", "delay-force", "force", "make-promise", "promise?"]),
    (&["scheme", "load"], &["load"]),
    (&["scheme", "process-context"], &[
        "command-line", "emergency-exit", "exit", "get-environment-variable", "get-environment-variables",
    ]),
    (&["scheme", "read"], &["read"]),
    (&["scheme", "repl"], &["interaction-environment"]),
    (&["scheme", "time"], &["current-jiffy", "current-second", "jiffies-per-second"]),
    (&["scheme", "write"], &["display", "write", "write-shared", "write-simple"]),
    (&["scheme", "r5rs"], &[
        "*", "+", "-", "/", "<", "<=", "=", ">", ">=", "abs", "acos", "and", "angle", "append", "apply",
        "asin", "assoc", "assq", "assv", "atan", "begin", "boolean?", "caaaar", "caaadr", "caaar",
        "caadar", "caaddr", "caadr", "caar", "cadaar", "cadadr", "cadar", "caddar", "cadddr", "caddr",
        "cadr", "call-with-current-continuation", "call-with-input-file", "call-with-output-file",
        "call-with-values", "car", "case", "cdaaar", "cdaadr", "cdaar", "cdadar", "cdaddr", "cdadr",
        "cdar", "cddaar", "cddadr", "cddar", "cdddar", "cddddr", "cdddr", "cddr", "cdr", "ceiling",
        "char->integer", "char-alphabetic?", "char-ci<=?", "char-ci<?", "char-ci=?", "char-ci>=?",
        "char-ci>?", "char-downcase", "char-lower-case?", "char-numeric?", "char-ready?", "char-upcase",
        "char-upper-case?", "char-whitespace?", "char<=?", "char<?", "char=?", "char>=?", "char>?",
        "char?", "close-input-port", "close-output-port", "complex?", "cond", "cons", "cos",
        "current-input-port", "current-output-port", "define", "define-syntax", "delay", "denominator",
        "display", "do", "dynamic-wind", "eof-object?", "eq?", "equal?", "eqv?", "eval", "even?",
        "exact->inexact", "exact?", "exp", "expt", "floor", "for-each", "force", "gcd", "if",
        "imag-part", "inexact->exact", "inexact?", "input-port?", "integer->char", "integer?",
        "interaction-environment", "lambda", "lcm", "length", "let", "let*", "let-syntax", "letrec",
        "letrec-syntax", "list", "list->string", "list->vector", "list-ref", "list-tail", "list?",
        "load", "log", "magnitude", "make-polar", "make-rectangular", "make-string", "make-vector",
        "map", "max", "member", "memq", "memv", "min", "modulo", "negative?", "newline", "not",
        "null-environment", "null?", "number->string", "number?", "numerator", "odd?",
        "open-input-file", "open-output-file", "or", "output-port?", "pair?", "peek-char",
        "positive?", "procedure?", "quasiquote", "quote", "quotient", "rational?", "rationalize",
        "read", "read-char", "real-part", "real?", "remainder", "reverse", "round",
        "scheme-report-environment", "set!", "set-car!", "set-cdr!", "sin", "sqrt", "string",
        "string->list", "string->number", "string->symbol", "string-append", "string-ci<=?",
        "string-ci<?", "string-ci=?", "string-ci>=?", "string-ci>?", "string-copy", "string-fill!",
        "string-length", "string-ref", "string-set!", "string<=?", "string<?", "string=?",
        "string>=?", "string>?", "string?", "substring", "symbol->string", "symbol?", "tan",
        "truncate", "values", "vector", "vector->list", "vector-fill!", "vector-length", "vector-ref",
        "vector-set!", "vector?", "with-input-from-file", "with-output-to-file", "write",
        "write-char", "zero?",
    ]),
];

/// Register the standard libraries, and `(s2)`, over the system
/// environment's bindings. Called once, after `scheme/core.scm` has loaded.
pub fn register_standard_libraries(heap: &mut Heap, system: GcRef) {
    let bindings: FxHashMap<GcRef, TopBinding> = env_of(system).borrow().bindings().collect();
    let mut standard = FxHashSet::default();
    for (name, members) in STANDARD_LIBRARIES {
        let mut exports = Vec::new();
        let mut unimplemented = Vec::new();
        for &member in *members {
            let symbol = heap.intern(member).as_ref().unwrap();
            standard.insert(symbol);
            match bindings.get(&symbol) {
                Some(&binding) if bound(binding) => exports.push((symbol, binding)),
                _ => unimplemented.push(member),
            }
        }
        let name = name.iter().map(|part| part.to_string()).collect();
        heap.libraries.insert(name, Library { exports, env: None, unimplemented });
    }
    // (s2): everything else, in name order. Not the hidden definitions
    // that macros make under aliases (`define-record-type`'s `index`).
    let mut rest: Vec<(GcRef, TopBinding)> = bindings
        .into_iter()
        .filter(|&(id, binding)| {
            !standard.contains(&id) && bound(binding) && !matches!(kind(Value::obj(id)), Some(Obj::Alias { .. }))
        })
        .collect();
    rest.sort_by_key(|&(symbol, _)| id_name(symbol).to_string());
    heap.libraries.insert(vec!["s2".to_string()], Library { exports: rest, env: None, unimplemented: Vec::new() });
}

/// Whether a binding means something: a variable that is unbound (a cell
/// made only because something referred to it) doesn't count.
fn bound(binding: TopBinding) -> bool {
    match binding {
        TopBinding::Variable(cell) => !cell_value(cell).get().is_undefined(),
        _ => true,
    }
}

/// Parse a library name datum such as `(scheme base)`.
pub fn library_name(datum: Value, who: &str) -> Result<LibraryName, String> {
    let parts = list_to_vec(datum).unwrap_or_default();
    let names: Option<LibraryName> = parts
        .iter()
        .map(|&p| match (identifier(p), p.as_fixnum()) {
            (Some(id), _) => Some(id_name(id).to_string()),
            (None, Some(n)) if n >= 0 => Some(n.to_string()),
            _ => None,
        })
        .collect();
    match names {
        Some(names) if !names.is_empty() => Ok(names),
        _ => Err(format!("{}: not a library name: {}", who, print_value(datum))),
    }
}

/// A library name as written: `(scheme base)`.
pub fn format_name(name: &LibraryName) -> String {
    format!("({})", name.join(" "))
}

/// The library named `name`, loading its file if it has one that hasn't
/// been loaded: in that case the file is recorded in `pending_load` and the
/// result is an error, for the caller to load the file and try again.
fn find_library<'a>(heap: &'a mut Heap, name: &LibraryName, who: &str) -> Result<&'a Library, String> {
    if heap.libraries.get(name).is_none() {
        match find_library_file(name) {
            Some(path) if heap.libraries.loaded.contains(&path) => {
                return Err(format!("{}: {} does not define {}", who, path.display(), format_name(name)));
            }
            Some(path) => {
                let message = format!("{}: loading {}", who, path.display());
                heap.libraries.pending_load = Some(path);
                return Err(message);
            }
            None => return Err(format!("{}: unknown library {}", who, format_name(name))),
        }
    }
    Ok(heap.libraries.get(name).unwrap())
}

/// Check that library `name` is registered. If it isn't but its file can
/// be found, the error leaves `pending_load` set, as for an import.
pub fn require_library(heap: &mut Heap, name: &LibraryName) -> Result<(), String> {
    find_library(heap, name, "load").map(|_| ())
}

/// The (name, binding) pairs an import set denotes (R7RS §5.2): a library
/// name, or `only`, `except`, `prefix` or `rename` applied to an import
/// set. Identifiers are compared by name, so an import made by a macro
/// works.
pub fn resolve_import_set(heap: &mut Heap, set: Value, who: &str) -> Result<Vec<(GcRef, TopBinding)>, String> {
    let parts = list_to_vec(set).ok_or_else(|| format!("{}: not an import set: {}", who, print_value(set)))?;
    let keyword = parts.first().and_then(|&h| identifier(h)).map(id_name);
    let modifier = matches!(keyword, Some("only" | "except" | "prefix" | "rename")) && parts.len() >= 2;
    if !modifier {
        let name = library_name(set, who)?;
        return Ok(find_library(heap, &name, who)?.exports.clone());
    }
    let mut bindings = resolve_import_set(heap, parts[1], who)?;
    let args = &parts[2..];
    let position = |bindings: &Vec<(GcRef, TopBinding)>, id: Value| -> Result<usize, String> {
        let name = identifier(id).map(id_name).unwrap_or("");
        bindings
            .iter()
            .position(|&(n, _)| id_name(n) == name)
            .ok_or_else(|| format!("{}: {} is not in the import set {}", who, name, print_value(parts[1])))
    };
    match keyword.unwrap() {
        "only" => {
            let mut kept = Vec::with_capacity(args.len());
            for &id in args {
                kept.push(bindings[position(&bindings, id)?]);
            }
            bindings = kept;
        }
        "except" => {
            for &id in args {
                let i = position(&bindings, id)?;
                bindings.remove(i);
            }
        }
        "prefix" => {
            let [prefix] = args else { return Err(format!("{}: prefix takes an import set and one identifier", who)) };
            let prefix = identifier(*prefix).map(id_name).unwrap_or("").to_string();
            for (name, _) in bindings.iter_mut() {
                *name = heap.intern(&format!("{}{}", prefix, id_name(*name))).as_ref().unwrap();
            }
        }
        _ => {
            for &pair in args {
                let ids = list_to_vec(pair).unwrap_or_default();
                let [from, to] = ids[..] else { return Err(format!("{}: rename expects (from to) pairs", who)) };
                let i = position(&bindings, from)?;
                let to = identifier(to).map(id_name).unwrap_or("").to_string();
                bindings[i].0 = heap.intern(&to).as_ref().unwrap();
            }
        }
    }
    Ok(bindings)
}

// ---------------------------------------------------------------------------
// define-library declarations and include

/// What a `define-library`'s declarations add up to.
#[derive(Default)]
pub struct Declarations {
    /// (internal, external) names.
    pub exports: Vec<(Value, Value)>,
    /// The import sets, in order.
    pub imports: Vec<Value>,
    /// The body forms, from `begin` and `include` declarations, in order.
    pub body: Vec<Value>,
}

/// Add the library declarations `decls` to `out`, expanding
/// `include-library-declarations` and `cond-expand`. Declarations are
/// recognised by name, as in s1: they aren't bindings anywhere.
pub fn collect_declarations(heap: &mut Heap, decls: &[Value], out: &mut Declarations) -> Result<(), String> {
    let who = "define-library";
    for &decl in decls {
        let parts = list_to_vec(decl).ok_or_else(|| format!("{}: not a declaration: {}", who, print_value(decl)))?;
        let keyword = parts.first().and_then(|&k| identifier(k)).map(id_name).unwrap_or("");
        let args = &parts[1.min(parts.len())..];
        match keyword {
            "export" => {
                for &spec in args {
                    if identifier(spec).is_some() {
                        out.exports.push((spec, spec));
                        continue;
                    }
                    match list_to_vec(spec).as_deref() {
                        Some(&[kw, internal, external]) if identifier(kw).map(id_name) == Some("rename") => {
                            out.exports.push((internal, external))
                        }
                        _ => return Err(format!("{}: bad export spec {}", who, print_value(spec))),
                    }
                }
            }
            "import" => out.imports.extend_from_slice(args),
            "begin" => out.body.extend_from_slice(args),
            "include" | "include-ci" => out.body.extend(include_forms(heap, args, keyword == "include-ci", keyword)?),
            "include-library-declarations" => {
                let forms = include_forms(heap, args, false, keyword)?;
                collect_declarations(heap, &forms, out)?;
            }
            "cond-expand" => {
                let chosen = choose_cond_expand_clause(heap, args, who)?;
                collect_declarations(heap, &chosen, out)?;
            }
            _ => return Err(format!("{}: unknown declaration {}", who, print_value(decl))),
        }
    }
    Ok(())
}

/// Every datum in the file `name`, read with case folding for
/// `include-ci`. Relative names are relative to the current directory.
pub fn read_file_forms(heap: &mut Heap, name: &str, fold_case: bool, who: &str) -> Result<Vec<Value>, String> {
    if let Some(reading) = &mut heap.libraries.reading {
        reading.push(name.into());
    }
    let content = std::fs::read_to_string(name).map_err(|e| format!("{}: could not read {}: {}", who, name, e))?;
    let mut port = new_string_port_input(&content);
    if let PortKind::StringPortInput { fold_case: fold, .. } = &port {
        fold.set(fold_case);
    }
    let mut forms = Vec::new();
    loop {
        match parse(heap, &mut port) {
            Ok(form) => forms.push(form),
            Err(ParseError::Eof) => return Ok(forms),
            Err(ParseError::Syntax(e)) => return Err(format!("{}: {}: {}", who, name, e)),
        }
    }
}

/// The forms of the files named by `files`, strings, in order: what
/// `include` and `include-ci` splice in.
pub fn include_forms(heap: &mut Heap, files: &[Value], fold_case: bool, who: &str) -> Result<Vec<Value>, String> {
    if files.is_empty() {
        return Err(format!("{}: expects at least one file name", who));
    }
    let mut forms = Vec::new();
    for &file in files {
        let Some(Obj::Str(name)) = kind(file) else { return Err(format!("{}: file names must be strings", who)) };
        let name = name.borrow().to_string();
        forms.extend(read_file_forms(heap, &name, fold_case, who)?);
    }
    Ok(forms)
}

// ---------------------------------------------------------------------------
// Library files

/// The directories searched for library files, in order: those in
/// `S2_LIBRARY_PATH` (separated as `PATH` is), the current directory, and
/// the installation's `<prefix>/share/s2/lib` for a binary in
/// `<prefix>/bin`.
// ponytail: s1 also searches the user's data directory, through the `dirs`
// crate; add it if anyone installs libraries per user.
fn search_path() -> Vec<PathBuf> {
    let mut path: Vec<PathBuf> = std::env::var_os("S2_LIBRARY_PATH")
        .map(|p| std::env::split_paths(&p).filter(|d| !d.as_os_str().is_empty()).collect())
        .unwrap_or_default();
    path.push(".".into());
    if let Some(prefix) = std::env::current_exe().ok().and_then(|exe| exe.parent()?.parent().map(Path::to_path_buf)) {
        path.push(prefix.join("share").join("s2").join("lib"));
    }
    path
}

/// The first file on the search path that could define library `name`:
/// `(foo bar)` is `foo/bar.sld`.
pub fn find_library_file(name: &LibraryName) -> Option<PathBuf> {
    let mut file: PathBuf = name.iter().collect();
    file.set_extension("sld");
    search_path().into_iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
}

/// A library file's forms, for `%load-library`, with the relative file
/// names in its `define-library`s' include declarations joined to the
/// file's directory.
fn read_library_file(heap: &mut Heap, path: &str) -> Result<Vec<Value>, String> {
    let dir = Path::new(path).parent().map(Path::to_path_buf).unwrap_or_default();
    let forms = read_file_forms(heap, path, false, "import")?;
    Ok(forms.into_iter().map(|form| relocate_includes(heap, form, &dir)).collect())
}

/// The compiled code of the library file `path`, from `foo/bar.s2o` next
/// to `foo/bar.sld` if that is fresh (`fasl::fresh`), or else compiled now
/// and cached there. Compiling doesn't run anything, so the file's forms
/// are expanded before any of them runs. Writing the cache is best effort.
fn compiled_library(heap: &mut Heap, path: &str) -> Result<Vec<GcRef>, String> {
    let target = heap.interaction.unwrap();
    let cache = Path::new(path).with_extension("s2o");
    if let Some(templates) = fasl::read_cached(heap, target, &cache)? {
        return Ok(templates);
    }
    heap.libraries.reading = Some(Vec::new());
    // A program being compiled may be recording its own changes to the
    // same environment: they wait while this file is compiled.
    let outer = env_of(target).borrow_mut().touched.replace(Vec::new());
    let result = (|| {
        let mut templates = Vec::new();
        for form in read_library_file(heap, path)? {
            let ast = expand(heap, target, form)?;
            templates.push(generate(heap, &ast));
        }
        Ok(templates)
    })();
    let sources = heap.libraries.reading.take().unwrap_or_default();
    if let Ok(templates) = &result
        && let Ok(bytes) = fasl::write(heap, target, templates, &sources)
    {
        fasl::write_cache(&cache, &bytes);
    }
    env_of(target).borrow_mut().touched = outer;
    result
}

/// `form`, if it is a `define-library`, with its include declarations'
/// relative file names joined to `dir`; otherwise `form` itself.
fn relocate_includes(heap: &mut Heap, form: Value, dir: &Path) -> Value {
    let Some(parts) = list_to_vec(form) else { return form };
    if parts.len() < 2 || parts[0].as_ref().map(id_name) != Some("define-library") {
        return form;
    }
    let mut new_parts = parts[..2].to_vec();
    for &decl in &parts[2..] {
        let decl_parts = list_to_vec(decl).unwrap_or_default();
        let is_include = decl_parts
            .first()
            .and_then(|&k| identifier(k))
            .is_some_and(|k| matches!(id_name(k), "include" | "include-ci" | "include-library-declarations"));
        if !is_include {
            new_parts.push(decl);
            continue;
        }
        let mut relocated = vec![decl_parts[0]];
        for &file in &decl_parts[1..] {
            match kind(file) {
                Some(Obj::Str(name)) if Path::new(name.borrow().as_str()).is_relative() => {
                    let joined = dir.join(name.borrow().as_str()).to_string_lossy().into_owned();
                    relocated.push(heap.string(&joined));
                }
                _ => relocated.push(file),
            }
        }
        new_parts.push(heap.list(relocated));
    }
    heap.list(new_parts)
}

// ---------------------------------------------------------------------------
// cond-expand and features

/// s2's feature identifiers (R7RS appendix B).
fn feature_names() -> Vec<String> {
    let mut names: Vec<String> = ["r7rs", "exact-closed", "ieee-float", "full-unicode", "ratios", "s2"].iter().map(|f| f.to_string()).collect();
    names.push(std::env::consts::OS.to_string());
    names.push(std::env::consts::FAMILY.to_string());
    if std::env::consts::FAMILY == "unix" {
        names.push("posix".to_string());
    }
    // R7RS spells architectures with hyphens: x86-64, not x86_64.
    names.push(std::env::consts::ARCH.replace('_', "-"));
    names.push(if cfg!(target_endian = "little") { "little-endian" } else { "big-endian" }.to_string());
    names
}

/// Whether a cond-expand requirement holds: a feature identifier,
/// `(library name)` for a library that is registered or has a file on the
/// search path, or `and` / `or` / `not` of requirements.
fn requirement_holds(heap: &Heap, req: Value, features: &[String], who: &str) -> Result<bool, String> {
    let bad = || format!("{}: bad feature requirement {}", who, print_value(req));
    if let Some(id) = identifier(req) {
        return Ok(features.iter().any(|f| f == id_name(id)));
    }
    let parts = list_to_vec(req).filter(|p| !p.is_empty()).ok_or_else(bad)?;
    let args = &parts[1..];
    match identifier(parts[0]).map(id_name) {
        Some("and") => {
            for &r in args {
                if !requirement_holds(heap, r, features, who)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Some("or") => {
            for &r in args {
                if requirement_holds(heap, r, features, who)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Some("not") => match args {
            [r] => Ok(!requirement_holds(heap, *r, features, who)?),
            _ => Err(bad()),
        },
        Some("library") => match args {
            [name] => {
                let name = library_name(*name, who)?;
                Ok(heap.libraries.get(&name).is_some() || find_library_file(&name).is_some())
            }
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}

/// The body of the first cond-expand clause whose requirement holds (an
/// `else` clause, last, always does), or nothing if none does.
pub fn choose_cond_expand_clause(heap: &Heap, clauses: &[Value], who: &str) -> Result<Vec<Value>, String> {
    let features = feature_names();
    for (i, &clause) in clauses.iter().enumerate() {
        let parts = list_to_vec(clause).unwrap_or_default();
        let Some(&req) = parts.first() else { return Err(format!("{}: bad clause {}", who, print_value(clause))) };
        let holds = if identifier(req).map(id_name) == Some("else") {
            if i + 1 != clauses.len() {
                return Err(format!("{}: else must be the last clause", who));
            }
            true
        } else {
            requirement_holds(heap, req, &features, who)?
        };
        if holds {
            return Ok(parts[1..].to_vec());
        }
    }
    Ok(Vec::new())
}

// ---------------------------------------------------------------------------
// Builtins

pub static PRIMITIVES: &[Primitive] = &[
    prim("environment", 0, None, |h, a| {
        let mut bindings = Vec::new();
        for &set in a {
            let resolved = resolve_import_set(h, set, "environment");
            // environment doesn't load library files (as in s1).
            h.libraries.pending_load = None;
            bindings.extend(resolved?);
        }
        Ok(environment_value(h, bindings))
    }),
    prim("interaction-environment", 0, Some(0), |h, _| Ok(Value::obj(h.interaction.unwrap()))),
    prim("scheme-report-environment", 1, Some(1), |h, a| {
        let bindings = r5rs_bindings(h, a[0], "scheme-report-environment")?;
        Ok(environment_value(h, bindings))
    }),
    prim("null-environment", 1, Some(1), |h, a| {
        let mut bindings = r5rs_bindings(h, a[0], "null-environment")?;
        bindings.retain(|&(_, b)| !matches!(b, TopBinding::Variable(_)));
        Ok(environment_value(h, bindings))
    }),
    prim("environment?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Environment(_)))))),
    prim("features", 0, Some(0), |h, _| {
        let symbols = feature_names().iter().map(|f| h.intern(f)).collect();
        Ok(h.list(symbols))
    }),
    prim("library-exports", 1, Some(1), |h, a| {
        let name = library_name(a[0], "library-exports")?;
        let library = h.libraries.get(&name).ok_or_else(|| err!("library-exports: unknown library {}", format_name(&name)))?;
        let names = library.exports.iter().map(|&(n, _)| Value::obj(n)).collect();
        Ok(h.list(names))
    }),
    prim("library-names", 0, Some(0), |h, _| {
        let mut names = Vec::new();
        for name in h.libraries.names() {
            let parts = name.iter().map(|p| p.parse::<i64>().ok().and_then(Value::fixnum).unwrap_or_else(|| h.intern(p))).collect();
            names.push(h.list(parts));
        }
        Ok(h.list(names))
    }),
    prim("%library-unimplemented", 1, Some(1), |h, a| {
        let name = library_name(a[0], "%library-unimplemented")?;
        let missing = h.libraries.get(&name).map(|l| l.unimplemented.clone()).unwrap_or_default();
        let strings = missing.iter().map(|m| h.string(m)).collect();
        Ok(h.list(strings))
    }),
    // (%register-library 'name '((internal . external) ...) env): the last
    // step of a define-library body. It checks every export is defined,
    // and only then registers the library, so one whose body failed isn't.
    prim("%register-library", 3, Some(3), register_library),
    // What %load-library needs: mark a library file loaded (before it
    // runs, so a library that imports itself can't loop), and read it.
    prim("%library-file-loading", 1, Some(1), |h, a| {
        let path = string_arg(a[0], "%library-file-loading")?.borrow().to_string();
        h.libraries.loaded.insert(PathBuf::from(path));
        Ok(Value::UNSPECIFIED)
    }),
    // For %load-library: the library file's code, compiled, as a list of
    // procedures to call in order; or the file of a library it needs
    // loaded first; or #f if it can't be compiled ahead.
    prim("%compiled-library", 1, Some(1), |h, a| {
        let path = string_arg(a[0], "%compiled-library")?.borrow().to_string();
        Ok(match compiled_library(h, &path) {
            Ok(templates) => {
                let thunks = templates.into_iter().map(|t| h.closure(t, std::iter::empty())).collect();
                h.list(thunks)
            }
            Err(_) => match h.libraries.pending_load.take() {
                Some(needed) => h.string(&needed.to_string_lossy()),
                None => Value::FALSE,
            },
        })
    }),
    prim("%read-library-file", 1, Some(1), |h, a| {
        let path = string_arg(a[0], "%read-library-file")?.borrow().to_string();
        let forms = read_library_file(h, &path)?;
        Ok(h.list(forms))
    }),
    // For load: every datum in a file.
    prim("%read-file-forms", 1, Some(1), |h, a| {
        let path = string_arg(a[0], "load")?.borrow().to_string();
        let forms = read_file_forms(h, &path, false, "load")?;
        Ok(h.list(forms))
    }),
];

/// A new immutable environment holding `bindings`.
fn environment_value(heap: &mut Heap, bindings: Vec<(GcRef, TopBinding)>) -> Value {
    let mut env = Env::empty(false);
    for (name, binding) in bindings {
        env.import(name, binding);
    }
    Value::obj(heap.environment(env))
}

/// The bindings of `(scheme r5rs)`, for an R5RS environment of `version`,
/// which must be 5.
fn r5rs_bindings(heap: &Heap, version: Value, who: &str) -> Result<Vec<(GcRef, TopBinding)>, Error> {
    if version.as_fixnum() != Some(5) {
        return Err(err!("{}: the only version supported is 5", who));
    }
    let name = vec!["scheme".to_string(), "r5rs".to_string()];
    Ok(heap.libraries.get(&name).map(|l| l.exports.clone()).unwrap_or_default())
}

fn register_library(heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let name = library_name(args[0], "define-library")?;
    let Some(Obj::Environment(env)) = kind(args[2]) else { return Err(expected("%register-library", "an environment", args[2])) };
    let mut exports = Vec::new();
    for pair in list_arg(args[1], "%register-library")? {
        let Some(Obj::Pair { car, cdr }) = kind(pair) else { return Err("%register-library: bad export list".into()) };
        let internal = car.get().as_ref().unwrap();
        let binding = env.borrow().lookup(internal).filter(|&b| bound(b));
        let Some(binding) = binding else {
            return Err(err!("define-library: {} exports {}, which it doesn't define", format_name(&name), id_name(internal)));
        };
        // The external name is what importers write: a plain symbol, even
        // if a macro produced the declaration.
        exports.push((strip(cdr.get().as_ref().unwrap()), binding));
    }
    heap.libraries.insert(name, Library { exports, env: Some(args[2].as_ref().unwrap()), unimplemented: Vec::new() });
    Ok(Value::UNSPECIFIED)
}
