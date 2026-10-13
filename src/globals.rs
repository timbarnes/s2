//! Top-level environments and their binding cells (D7, plan/03 §8, §13).
//!
//! An environment maps each top-level identifier to what it means: a
//! variable, a macro, or a core form of the expander (plan/03 §12). It is a
//! heap object (`Obj::Environment`), so it can be a Scheme value, the
//! argument of `eval`, and so a macro can remember the environment it was
//! defined in.
//!
//! A variable is a binding cell, a heap object holding the variable's
//! value. The compiler puts the cell itself into a template's constant
//! pool, so a global reference at run time is one load from the cell, with
//! no lookup by name. Redefining a variable changes the same cell, so code
//! compiled earlier sees the new value (late binding).
//!
//! There are three kinds of environment, as in s1
//! (`design/libraries-design.md`):
//!
//! - **the system environment**: the builtins, the core forms, and what
//!   `scheme/core.scm` defines. The standard libraries export its bindings.
//! - **the interaction environment**, where the REPL and programs run. It
//!   starts with every system binding imported.
//! - **library environments**, one per `define-library`, and the immutable
//!   environments `environment` makes.
//!
//! An imported binding is shared with the library it came from: the same
//! cell, so the importer sees the library's assignments. `define` of an
//! imported name gives it a fresh local cell instead (the library's is
//! untouched), and `set!` of one is an error.
//!
//! The keys are identifiers: usually symbols, but a top-level definition
//! introduced by a macro expansion is keyed by its alias, so only that
//! expansion can see it (as in s1).

use rustc_hash::FxHashMap;

use crate::heap::{GcRef, Heap, Obj};
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
    Import,
    DefineLibrary,
    Include,
    IncludeCi,
    CondExpand,
    SyntaxError,
    /// Auxiliary syntax (R7RS §4.3.2): `else`, `=>`, `_` and `...`. They are
    /// bound so that they can be exported, imported and renamed, and so
    /// that a macro's literal matches them by binding: `cond` recognises
    /// an imported `else` renamed `otherwise`, and not a local variable
    /// named `else`. Used as an expression, they are an error.
    Auxiliary,
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
    ("import", Core::Import),
    ("define-library", Core::DefineLibrary),
    ("include", Core::Include),
    ("include-ci", Core::IncludeCi),
    ("cond-expand", Core::CondExpand),
    ("syntax-error", Core::SyntaxError),
    ("else", Core::Auxiliary),
    ("=>", Core::Auxiliary),
    ("_", Core::Auxiliary),
    ("...", Core::Auxiliary),
];

#[derive(Clone, Copy, PartialEq)]
pub enum TopBinding {
    /// A variable: its binding cell.
    Variable(GcRef),
    /// A macro: an `Obj::Macro`.
    Macro(GcRef),
    Core(Core),
}

/// A top-level environment.
pub struct Env {
    /// Each identifier's binding, and whether it was imported.
    bindings: FxHashMap<GcRef, (TopBinding, bool)>,
    /// False for the environments `environment` makes: they can't be
    /// defined in, assigned to or imported into.
    pub mutable: bool,
    /// While a file is compiled ahead (`fasl.rs`): every identifier whose
    /// binding changed, so the compiled file can make the same changes.
    pub touched: Option<Vec<GcRef>>,
}

/// The `Env` of an `Obj::Environment`.
pub fn env_of<'a>(env: GcRef) -> &'a std::cell::RefCell<Env> {
    match &env.get().kind {
        Obj::Environment(e) => e,
        _ => unreachable!("not an environment"),
    }
}

impl Env {
    /// An empty environment: a library's, before its imports.
    pub fn empty(mutable: bool) -> Env {
        Env { bindings: FxHashMap::default(), mutable, touched: None }
    }

    /// The system environment's start: the core forms.
    pub fn with_core_forms(heap: &mut Heap) -> Env {
        let mut env = Env::empty(true);
        for &(name, core) in CORE_FORMS {
            let sym = heap.intern(name).as_ref().unwrap();
            env.bindings.insert(sym, (TopBinding::Core(core), false));
        }
        env
    }

    pub fn lookup(&self, id: GcRef) -> Option<TopBinding> {
        self.bindings.get(&id).map(|&(b, _)| b)
    }

    pub fn is_imported(&self, id: GcRef) -> bool {
        self.bindings.get(&id).is_some_and(|&(_, imported)| imported)
    }

    /// Every binding, for importing all of them (the interaction
    /// environment) and for the standard libraries' export tables.
    pub fn bindings(&self) -> impl Iterator<Item = (GcRef, TopBinding)> + '_ {
        self.bindings.iter().map(|(&k, &(b, _))| (k, b))
    }

    /// The cell for a reference to the variable `name`: the variable's own
    /// if there is one, or a new unbound one. Compiling a reference to a
    /// variable that is defined later (or never) therefore works; the error
    /// comes when the code runs. In an immutable environment the new cell
    /// isn't added, so it stays unbound.
    pub fn cell(&mut self, heap: &mut Heap, name: GcRef) -> GcRef {
        match self.bindings.get(&name) {
            Some(&(TopBinding::Variable(cell), _)) => cell,
            _ => {
                let cell = heap.global(name);
                if self.mutable {
                    self.bind(name, TopBinding::Variable(cell), false);
                }
                cell
            }
        }
    }

    /// The cell a top-level `define` of `name` assigns: the variable's own,
    /// unless it is imported (or a macro or core form), which gets a fresh
    /// local cell.
    pub fn define_cell(&mut self, heap: &mut Heap, name: GcRef) -> Result<GcRef, String> {
        if !self.mutable {
            return Err("define: this environment is immutable".into());
        }
        Ok(self.own_cell(heap, name, true).unwrap())
    }

    /// The cell `set!` of the global `name` assigns. Assigning an imported
    /// variable is an error (R7RS §5.6).
    pub fn set_cell(&mut self, heap: &mut Heap, name: GcRef) -> Result<GcRef, String> {
        if self.is_imported(name) {
            return Err(format!("set!: {} is imported and can't be assigned", print_value(Value::obj(name))));
        }
        Ok(self.cell(heap, name))
    }

    /// Bind `name` to the macro `m`, as a top-level `define-syntax` does.
    pub fn define_syntax(&mut self, name: GcRef, m: GcRef) -> Result<(), String> {
        if !self.mutable {
            return Err("define-syntax: this environment is immutable".into());
        }
        self.bind(name, TopBinding::Macro(m), false);
        Ok(())
    }

    /// Bind `name` to `binding`, as `import` does. It replaces any binding
    /// `name` had.
    pub fn import(&mut self, name: GcRef, binding: TopBinding) {
        self.bind(name, binding, true);
    }

    /// Bind `name`, replacing any binding it had.
    pub fn bind(&mut self, name: GcRef, binding: TopBinding, imported: bool) {
        if let Some(touched) = &mut self.touched {
            touched.push(name);
        }
        self.bindings.insert(name, (binding, imported));
    }

    /// `name`'s binding, and whether it was imported.
    pub fn entry(&self, name: GcRef) -> Option<(TopBinding, bool)> {
        self.bindings.get(&name).copied()
    }

    /// The cell of the variable `name` defines here (not one it imports).
    /// If there is none, `create` makes one, replacing any other binding.
    pub fn own_cell(&mut self, heap: &mut Heap, name: GcRef, create: bool) -> Option<GcRef> {
        match self.bindings.get(&name) {
            Some(&(TopBinding::Variable(cell), false)) => Some(cell),
            _ if create => {
                let cell = heap.global(name);
                self.bind(name, TopBinding::Variable(cell), false);
                Some(cell)
            }
            _ => None,
        }
    }

    /// What the GC must keep: every key (aliases among them) and every
    /// cell and macro.
    pub fn trace(&self, mut visit: impl FnMut(GcRef)) {
        for (&k, &(b, _)) in &self.bindings {
            visit(k);
            if let TopBinding::Variable(r) | TopBinding::Macro(r) = b {
                visit(r);
            }
        }
    }

    /// Bind `name` to `value` as a builtin is: a local variable.
    pub fn define(&mut self, heap: &mut Heap, name: &str, value: Value) {
        let sym = heap.intern(name).as_ref().unwrap();
        cell_value(self.define_cell(heap, sym).unwrap()).set(value);
    }
}

/// The value slot of a binding cell.
pub fn cell_value<'a>(cell: GcRef) -> &'a std::cell::Cell<Value> {
    match &cell.get().kind {
        Obj::Global { value, .. } => value,
        _ => unreachable!("not a binding cell"),
    }
}

/// The identifier a binding cell belongs to.
pub fn cell_name(cell: GcRef) -> Value {
    match &cell.get().kind {
        Obj::Global { name, .. } => Value::obj(*name),
        _ => unreachable!("not a binding cell"),
    }
}
