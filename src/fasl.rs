//! Compiled files ("fasl", for fast load, as Chez calls them): a file's
//! compiled code, written so another process can run it without the
//! expander or code generator (plan/04 P11).
//!
//! **What compiled code depends on.** A template is plain data (`Op`s,
//! names) plus its constants: literals, inner templates, and *binding
//! cells*. A cell belongs to a top-level environment, where it is some
//! identifier's variable. A new process has its own cells, so a cell is
//! written as (environment, identifier) and found again when the file is
//! loaded:
//!
//! - in the environment the file is loaded into (the *target*), or one the
//!   file itself creates (a `define-library`'s), the cell is found or made;
//! - in the system environment or a library loaded from elsewhere, it must
//!   already exist. Those libraries are listed in the header, so the
//!   loader can load them first.
//!
//! **What compiling did besides making code.** Expanding a file changes
//! environments: `import`, `define-syntax` and `define` bind identifiers
//! while the forms are expanded, before any of them runs. The compiled
//! file records the bindings that changed (`Env::touched`), and a library
//! environment's whole contents, and loading makes them again: before the
//! code runs, so a compiled file's imports, macros and definitions are in
//! place from the start.
//!
//! **Macros** are stored as their parsed `syntax-rules` (literals, rules,
//! definition scope), not re-parsed from source. Aliases, the renamed
//! identifiers of macro expansions, are written as objects, so two
//! references to one alias are one alias again after loading: a hidden
//! definition and its uses still find the same cell.
//!
//! **The format.** A header (magic, version, the source files and their
//! modification times, the libraries needed), then an object table: each
//! object refers only to objects before it, by index, so reading is one
//! pass. Environments refer to their bindings, which can refer back to the
//! environment (a macro remembers where it was defined), so environments
//! are written empty, and their bindings in a section after all the
//! objects. Numbers are LEB128 varints; signed ones are zigzag-encoded.
//! Instructions are decoded to `Op`s on load: the VM runs `Op`s, not bytes.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use num_bigint::BigInt;
use num_rational::BigRational;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::compile::expand::CORE_FORMS;
use crate::compile::syntax_rules::SyntaxRules;
use crate::globals::{Env, TopBinding, env_of};
use crate::heap::{GcRef, Heap, Obj};
use crate::libraries::{LibraryName, require_library};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::op::Test;
use crate::vm::{Op, Template};

const MAGIC: &[u8; 8] = b"s2fasl\r\n";

/// Bump when the encoding, or the meaning of an instruction, changes:
/// files written by another version are refused.
///
/// 2: fixnums are 48-bit (NaN-boxed values), so a version 1 file can hold
/// fixnums that no longer are.
const VERSION: u32 = 2;

/// Whether `bytes` are a compiled file (of any version).
pub fn is_fasl(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

// ---------------------------------------------------------------------------
// Writing

/// A value, as written: an immediate, or an index into the object table.
#[derive(Clone, Copy)]
enum Val {
    Fixnum(i64),
    Flonum(f64),
    Char(char),
    Bool(bool),
    Nil,
    Unspecified,
    Eof,
    Undefined,
    Obj(u32),
}

// Object tags.
const PAIR: u8 = 0;
const SYMBOL: u8 = 1;
const STRING: u8 = 2;
const VECTOR: u8 = 3;
const BYTEVECTOR: u8 = 4;
const BIGINT: u8 = 5;
const RATIONAL: u8 = 6;
const CELL: u8 = 7;
const ALIAS: u8 = 8;
const ENVIRONMENT: u8 = 9;
const MACRO: u8 = 10;
const TEMPLATE: u8 = 11;
const MACRO_REF: u8 = 12;

// Kinds of environment.
const TARGET: u8 = 0;
const SYSTEM: u8 = 1;
const LIBRARY: u8 = 2;
const NEW: u8 = 3;

/// Compile-time state for writing: the templates of a file's forms,
/// compiled for `target`, whose `touched` list says which of its bindings
/// compiling changed. `sources` are the files the code came from, for
/// `fresh`.
pub fn write(heap: &Heap, target: GcRef, templates: &[GcRef], sources: &[PathBuf]) -> Result<Vec<u8>, String> {
    let mut w = Writer {
        heap,
        target,
        out: Vec::new(),
        index: FxHashMap::default(),
        count: 0,
        active: FxHashSet::default(),
        home: FxHashMap::default(),
        created: FxHashSet::default(),
        pending_envs: Vec::new(),
        needs: Vec::new(),
    };
    w.find_homes(templates);
    let mut tops = Vec::new();
    for &t in templates {
        tops.push(w.obj(t)?);
    }
    // The target's changed bindings, then every new environment's
    // bindings. Writing them can add objects, and more new environments.
    let mut bindings = Vec::new();
    let mut groups = 1;
    let mut touched = env_of(target).borrow().touched.clone().unwrap_or_default();
    let mut seen = FxHashSet::default();
    touched.retain(|&k| seen.insert(k));
    let target_env = w.env(target)?;
    w.bindings(&mut bindings, target_env, target, &touched)?;
    while let Some((i, env)) = w.pending_envs.pop() {
        let keys: Vec<GcRef> = env_of(env).borrow().bindings().map(|(k, _)| k).collect();
        w.bindings(&mut bindings, i, env, &keys)?;
        groups += 1;
    }

    let mut file = MAGIC.to_vec();
    file.extend(VERSION.to_le_bytes());
    put_uint(&mut file, sources.len() as u64);
    for path in sources {
        put_str(&mut file, &path.to_string_lossy());
        let (secs, nanos) = mtime(path).unwrap_or((0, 0));
        put_uint(&mut file, secs);
        put_uint(&mut file, nanos as u64);
    }
    put_uint(&mut file, w.needs.len() as u64);
    for name in &w.needs {
        put_uint(&mut file, name.len() as u64);
        name.iter().for_each(|part| put_str(&mut file, part));
    }
    put_uint(&mut file, w.count as u64);
    file.extend(&w.out);
    put_uint(&mut file, groups);
    file.extend(bindings);
    put_uint(&mut file, tops.len() as u64);
    tops.iter().for_each(|&t| put_uint(&mut file, t as u64));
    Ok(file)
}

struct Writer<'h> {
    heap: &'h Heap,
    target: GcRef,
    /// The object table, as written so far.
    out: Vec<u8>,
    index: FxHashMap<GcRef, u32>,
    count: u32,
    /// Objects being written, to refuse circular constants.
    active: FxHashSet<GcRef>,
    /// Where each cell and macro is defined: its environment and name.
    home: FxHashMap<GcRef, (GcRef, GcRef)>,
    /// The environments the file's code creates: those of its
    /// `define-library`s, found among the templates' constants. Loading
    /// makes them again, even if compiling ran the form and registered
    /// the library.
    created: FxHashSet<GcRef>,
    /// New environments whose bindings are still to be written.
    pending_envs: Vec<(u32, GcRef)>,
    needs: Vec<LibraryName>,
}

impl Writer<'_> {
    /// Where each cell the code might use is a variable: in the target,
    /// the system environment, a registered library, or an environment
    /// among the templates' constants (a `define-library`'s).
    fn find_homes(&mut self, templates: &[GcRef]) {
        let mut envs = vec![self.target];
        envs.extend(self.heap.system);
        envs.extend(self.heap.libraries.environments());
        let mut stack: Vec<GcRef> = templates.to_vec();
        while let Some(t) = stack.pop() {
            let Obj::Template(t) = &t.get().kind else { continue };
            for &c in &t.consts {
                match c.as_obj().map(|o| &o.kind) {
                    Some(Obj::Template(_)) => stack.push(c.as_ref().unwrap()),
                    Some(Obj::Environment(_)) => {
                        self.created.insert(c.as_ref().unwrap());
                        envs.push(c.as_ref().unwrap());
                    }
                    _ => {}
                }
            }
        }
        for env in envs {
            for (key, binding) in env_of(env).borrow().bindings() {
                if let TopBinding::Variable(r) | TopBinding::Macro(r) = binding
                    && !env_of(env).borrow().is_imported(key)
                {
                    self.home.insert(r, (env, key));
                }
            }
        }
    }

    /// Whether loading makes `env`'s definitions, rather than finding
    /// them: the target, or an environment the file creates.
    fn owns(&self, env: GcRef) -> bool {
        env == self.target || self.created.contains(&env) || (Some(env) != self.heap.system && self.heap.libraries.name_of(env).is_none())
    }

    fn val(&mut self, v: Value) -> Result<Val, String> {
        Ok(if let Some(r) = v.as_ref() {
            Val::Obj(self.obj(r)?)
        } else if let Some(n) = v.as_fixnum() {
            Val::Fixnum(n)
        } else if let Some(f) = v.as_flonum() {
            Val::Flonum(f)
        } else if let Some(c) = v.as_char() {
            Val::Char(c)
        } else if let Some(b) = v.as_bool() {
            Val::Bool(b)
        } else if v.is_nil() {
            Val::Nil
        } else if v.is_eof() {
            Val::Eof
        } else if v.is_undefined() {
            Val::Undefined
        } else {
            Val::Unspecified
        })
    }

    /// The index of object `r`, writing it (after everything it refers
    /// to) if it isn't in the table yet.
    fn obj(&mut self, r: GcRef) -> Result<u32, String> {
        if let Some(&i) = self.index.get(&r) {
            return Ok(i);
        }
        if !self.active.insert(r) {
            return Err(format!("can't compile a circular constant: {}", print_value(Value::obj(r))));
        }
        let mut b = Vec::new();
        match &r.get().kind {
            Obj::Pair { car, cdr } => {
                let (a, d) = (self.val(car.get())?, self.val(cdr.get())?);
                b.push(PAIR);
                put_val(&mut b, a);
                put_val(&mut b, d);
            }
            Obj::Symbol(name) => {
                b.push(SYMBOL);
                put_str(&mut b, name);
            }
            Obj::Str(s) => {
                b.push(STRING);
                put_str(&mut b, s.borrow().as_str());
            }
            Obj::Vector(items) => {
                let items = items.iter().map(|c| self.val(c.get())).collect::<Result<Vec<_>, _>>()?;
                b.push(VECTOR);
                put_vals(&mut b, &items);
            }
            Obj::Bytevector(bytes) => {
                b.push(BYTEVECTOR);
                put_uint(&mut b, bytes.len() as u64);
                b.extend(bytes.iter().map(|c| c.get()));
            }
            Obj::BigInt(n) => {
                b.push(BIGINT);
                put_bigint(&mut b, n);
            }
            Obj::Rational(q) => {
                b.push(RATIONAL);
                put_bigint(&mut b, q.numer());
                put_bigint(&mut b, q.denom());
            }
            Obj::Global { name, .. } => {
                let Some(&(home, _)) = self.home.get(&r) else {
                    return Err(format!("can't compile a reference to {}: its binding has been replaced", print_value(Value::obj(*name))));
                };
                let (key, env) = (self.obj(*name)?, self.env(home)?);
                b.push(CELL);
                put_uint(&mut b, key as u64);
                put_uint(&mut b, env as u64);
            }
            // A macro defined elsewhere: found by name when loading.
            Obj::Macro(_) if self.home.get(&r).is_some_and(|&(env, _)| !self.owns(env)) => {
                let (env, key) = self.home[&r];
                let (key, env) = (self.obj(key)?, self.env(env)?);
                b.push(MACRO_REF);
                put_uint(&mut b, key as u64);
                put_uint(&mut b, env as u64);
            }
            Obj::Alias { original, depth, env } => {
                let (original, env) = (self.val(*original)?, self.env(*env)?);
                b.push(ALIAS);
                put_val(&mut b, original);
                put_uint(&mut b, *depth as u64);
                put_uint(&mut b, env as u64);
            }
            Obj::Environment(e) => {
                b.push(ENVIRONMENT);
                if r == self.target {
                    b.push(TARGET);
                } else if self.created.contains(&r) {
                    b.push(NEW);
                    b.push(e.borrow().mutable as u8);
                    self.pending_envs.push((self.count, r));
                } else if Some(r) == self.heap.system {
                    b.push(SYSTEM);
                } else if let Some(name) = self.heap.libraries.name_of(r) {
                    b.push(LIBRARY);
                    put_uint(&mut b, name.len() as u64);
                    name.iter().for_each(|part| put_str(&mut b, part));
                    if !self.needs.contains(&name) {
                        self.needs.push(name);
                    }
                } else {
                    b.push(NEW);
                    b.push(e.borrow().mutable as u8);
                    self.pending_envs.push((self.count, r));
                }
            }
            Obj::Macro(m) => {
                let ellipsis = m.ellipsis.map(|e| self.obj(e)).transpose()?;
                let literals = m.literals.iter().map(|&l| self.obj(l)).collect::<Result<Vec<_>, _>>()?;
                let mut rules = Vec::new();
                for &(pattern, template) in &m.rules {
                    rules.push(self.val(pattern)?);
                    rules.push(self.val(template)?);
                }
                let env = self.env(m.env)?;
                b.push(MACRO);
                put_uint(&mut b, ellipsis.map_or(0, |e| e as u64 + 1));
                put_uint(&mut b, literals.len() as u64);
                literals.iter().for_each(|&l| put_uint(&mut b, l as u64));
                put_vals(&mut b, &rules);
                put_uint(&mut b, m.depth as u64);
                put_uint(&mut b, env as u64);
            }
            Obj::Template(t) => {
                let consts = t.consts.iter().map(|&c| self.val(c)).collect::<Result<Vec<_>, _>>()?;
                b.push(TEMPLATE);
                put_str(&mut b, &t.name);
                put_uint(&mut b, t.required as u64);
                b.push(t.rest as u8);
                put_uint(&mut b, t.nfree as u64);
                put_uint(&mut b, t.code.len() as u64);
                t.code.iter().for_each(|&op| put_op(&mut b, op));
                put_vals(&mut b, &consts);
                for names in [&t.local_names, &t.free_names] {
                    put_uint(&mut b, names.len() as u64);
                    names.iter().for_each(|n| put_str(&mut b, n));
                }
            }
            _ => return Err(format!("can't compile a constant {}", print_value(Value::obj(r)))),
        }
        self.active.remove(&r);
        self.out.extend(b);
        let i = self.count;
        self.count += 1;
        self.index.insert(r, i);
        Ok(i)
    }

    fn env(&mut self, env: GcRef) -> Result<u32, String> {
        self.obj(env)
    }

    /// The bindings of `keys` in `env` (object `i`), into `out`.
    fn bindings(&mut self, out: &mut Vec<u8>, i: u32, env: GcRef, keys: &[GcRef]) -> Result<(), String> {
        let mut entries = Vec::new();
        for &key in keys {
            let Some((binding, imported)) = env_of(env).borrow().entry(key) else { continue };
            let k = self.obj(key)?;
            let b = match binding {
                TopBinding::Variable(c) => (0, self.obj(c)? as u64),
                TopBinding::Macro(m) => (1, self.obj(m)? as u64),
                TopBinding::Core(c) => (2, CORE_FORMS.iter().position(|&(_, f)| f == c).unwrap() as u64),
            };
            entries.push((k, b, imported));
        }
        put_uint(out, i as u64);
        put_uint(out, entries.len() as u64);
        for (k, (tag, x), imported) in entries {
            put_uint(out, k as u64);
            out.push(tag);
            put_uint(out, x);
            out.push(imported as u8);
        }
        Ok(())
    }
}

fn put_uint(b: &mut Vec<u8>, mut n: u64) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            b.push(byte);
            return;
        }
        b.push(byte | 0x80);
    }
}

fn put_int(b: &mut Vec<u8>, n: i64) {
    put_uint(b, ((n << 1) ^ (n >> 63)) as u64);
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    put_uint(b, s.len() as u64);
    b.extend(s.as_bytes());
}

fn put_bigint(b: &mut Vec<u8>, n: &BigInt) {
    let bytes = n.to_signed_bytes_le();
    put_uint(b, bytes.len() as u64);
    b.extend(bytes);
}

fn put_val(b: &mut Vec<u8>, v: Val) {
    match v {
        Val::Fixnum(n) => {
            b.push(0);
            put_int(b, n);
        }
        Val::Flonum(f) => {
            b.push(1);
            b.extend(f.to_le_bytes());
        }
        Val::Char(c) => {
            b.push(2);
            put_uint(b, c as u64);
        }
        Val::Bool(true) => b.push(3),
        Val::Bool(false) => b.push(4),
        Val::Nil => b.push(5),
        Val::Unspecified => b.push(6),
        Val::Eof => b.push(7),
        Val::Undefined => b.push(8),
        Val::Obj(i) => {
            b.push(9);
            put_uint(b, i as u64);
        }
    }
}

fn put_vals(b: &mut Vec<u8>, vals: &[Val]) {
    put_uint(b, vals.len() as u64);
    vals.iter().for_each(|&v| put_val(b, v));
}

/// The tests `JumpIfNot` makes, in encoding order.
const TESTS: [Test; 8] = [Test::NullP, Test::PairP, Test::EqP, Test::NumEq, Test::Lt, Test::Gt, Test::Le, Test::Ge];

/// An instruction: a tag byte, then its operands.
fn put_op(b: &mut Vec<u8>, op: Op) {
    let u = |b: &mut Vec<u8>, n: u64| put_uint(b, n);
    match op {
        Op::Const(k) => (b.push(0), u(b, k as u64)).1,
        Op::Fixnum(n) => (b.push(1), put_int(b, n as i64)).1,
        Op::Nil => b.push(2),
        Op::True => b.push(3),
        Op::False => b.push(4),
        Op::Unspecified => b.push(5),
        Op::Undefined => b.push(6),
        Op::LocalRef(i) => (b.push(7), u(b, i as u64)).1,
        Op::LocalSet(i) => (b.push(8), u(b, i as u64)).1,
        Op::FreeRef(i) => (b.push(9), u(b, i as u64)).1,
        Op::PatchFree(c, i, v) => (b.push(10), u(b, c as u64), u(b, i as u64), u(b, v as u64)).3,
        Op::MakeBox(i) => (b.push(11), u(b, i as u64)).1,
        Op::BoxRef => b.push(12),
        Op::BoxSet => b.push(13),
        Op::GlobalRef(k) => (b.push(14), u(b, k as u64)).1,
        Op::GlobalSet(k) => (b.push(15), u(b, k as u64)).1,
        Op::GlobalDefine(k) => (b.push(16), u(b, k as u64)).1,
        Op::Jump(t) => (b.push(17), u(b, t as u64)).1,
        Op::JumpIfFalse(t) => (b.push(18), u(b, t as u64)).1,
        Op::JumpIfNot(test, t) => {
            b.push(19);
            b.push(TESTS.iter().position(|&x| x == test).unwrap() as u8);
            u(b, t as u64);
        }
        Op::Pop => b.push(20),
        Op::Slide(n) => (b.push(21), u(b, n as u64)).1,
        Op::MakeClosure(k) => (b.push(22), u(b, k as u64)).1,
        Op::Call(n) => (b.push(23), u(b, n as u64)).1,
        Op::TailCall(n) => (b.push(24), u(b, n as u64)).1,
        Op::Return => b.push(25),
        Op::SelfTailCall(n) => (b.push(26), u(b, n as u64)).1,
        Op::Car => b.push(27),
        Op::Cdr => b.push(28),
        Op::Cons => b.push(29),
        Op::NullP => b.push(30),
        Op::PairP => b.push(31),
        Op::EqP => b.push(32),
        Op::Not => b.push(33),
        Op::Add => b.push(34),
        Op::Sub => b.push(35),
        Op::NumEq => b.push(36),
        Op::Lt => b.push(37),
        Op::Gt => b.push(38),
        Op::Le => b.push(39),
        Op::Ge => b.push(40),
    }
}

/// A file's modification time, as seconds and nanoseconds.
fn mtime(path: &Path) -> Option<(u64, u32)> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((t.as_secs(), t.subsec_nanos()))
}

// ---------------------------------------------------------------------------
// Reading

struct Reader<'b> {
    bytes: &'b [u8],
    at: usize,
}

const TRUNCATED: &str = "compiled file is truncated or damaged";

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let b = *self.bytes.get(self.at).ok_or(TRUNCATED)?;
        self.at += 1;
        Ok(b)
    }

    fn uint(&mut self) -> Result<u64, String> {
        let mut n = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            n |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(n);
            }
        }
        Err(TRUNCATED.into())
    }

    fn small<T: TryFrom<u64>>(&mut self) -> Result<T, String> {
        T::try_from(self.uint()?).map_err(|_| TRUNCATED.to_string())
    }

    fn int(&mut self) -> Result<i64, String> {
        let n = self.uint()?;
        Ok((n >> 1) as i64 ^ -((n & 1) as i64))
    }

    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or(TRUNCATED)?;
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn str(&mut self) -> Result<String, String> {
        let n = self.small()?;
        String::from_utf8(self.bytes(n)?.to_vec()).map_err(|_| TRUNCATED.to_string())
    }

    fn bigint(&mut self) -> Result<BigInt, String> {
        let n = self.small()?;
        Ok(BigInt::from_signed_bytes_le(self.bytes(n)?))
    }

    fn name(&mut self) -> Result<LibraryName, String> {
        let n = self.small()?;
        (0..n).map(|_| self.str()).collect()
    }

    /// The header up to the libraries needed: the sources and their
    /// modification times.
    fn header(&mut self) -> Result<Vec<(PathBuf, u64, u32)>, String> {
        if !is_fasl(self.bytes) {
            return Err("not a compiled file".into());
        }
        self.at = MAGIC.len();
        let version = u32::from_le_bytes(self.bytes(4)?.try_into().unwrap());
        if version != VERSION {
            return Err(format!("compiled file is version {}, and this s2 reads version {}: compile it again", version, VERSION));
        }
        let n = self.small()?;
        (0..n).map(|_| Ok((PathBuf::from(self.str()?), self.uint()?, self.small()?))).collect()
    }

    fn op(&mut self) -> Result<Op, String> {
        Ok(match self.u8()? {
            0 => Op::Const(self.small()?),
            1 => Op::Fixnum(i32::try_from(self.int()?).map_err(|_| TRUNCATED)?),
            2 => Op::Nil,
            3 => Op::True,
            4 => Op::False,
            5 => Op::Unspecified,
            6 => Op::Undefined,
            7 => Op::LocalRef(self.small()?),
            8 => Op::LocalSet(self.small()?),
            9 => Op::FreeRef(self.small()?),
            10 => Op::PatchFree(self.small()?, self.small()?, self.small()?),
            11 => Op::MakeBox(self.small()?),
            12 => Op::BoxRef,
            13 => Op::BoxSet,
            14 => Op::GlobalRef(self.small()?),
            15 => Op::GlobalSet(self.small()?),
            16 => Op::GlobalDefine(self.small()?),
            17 => Op::Jump(self.small()?),
            18 => Op::JumpIfFalse(self.small()?),
            19 => Op::JumpIfNot(*TESTS.get(self.u8()? as usize).ok_or(TRUNCATED)?, self.small()?),
            20 => Op::Pop,
            21 => Op::Slide(self.small()?),
            22 => Op::MakeClosure(self.small()?),
            23 => Op::Call(self.small()?),
            24 => Op::TailCall(self.small()?),
            25 => Op::Return,
            26 => Op::SelfTailCall(self.small()?),
            27 => Op::Car,
            28 => Op::Cdr,
            29 => Op::Cons,
            30 => Op::NullP,
            31 => Op::PairP,
            32 => Op::EqP,
            33 => Op::Not,
            34 => Op::Add,
            35 => Op::Sub,
            36 => Op::NumEq,
            37 => Op::Lt,
            38 => Op::Gt,
            39 => Op::Le,
            40 => Op::Ge,
            _ => return Err(TRUNCATED.into()),
        })
    }
}

/// Whether the compiled file `bytes`, written at `written`, is newer than
/// everything it was compiled from: each source still has the
/// modification time it had, and the running s2 was built before the file
/// was written (so a rebuilt s2, which may compile differently, compiles
/// again).
fn fresh(bytes: &[u8], written: SystemTime) -> bool {
    let Ok(sources) = (Reader { bytes, at: 0 }).header() else { return false };
    let exe_older = std::env::current_exe()
        .and_then(std::fs::metadata)
        .and_then(|m| m.modified())
        .is_ok_and(|built| built <= written);
    exe_older && sources.iter().all(|(path, secs, nanos)| mtime(path) == Some((*secs, *nanos)))
}

/// Load the compiled file at `path` into `target`, if it is fresh. None if
/// it is missing, stale or unreadable, for the caller to compile its
/// sources instead. An error only if it needs a library loaded first
/// (`pending_load` is set).
pub fn read_cached(heap: &mut Heap, target: GcRef, path: &Path) -> Result<Option<Vec<GcRef>>, String> {
    let Ok(bytes) = std::fs::read(path) else { return Ok(None) };
    let Ok(written) = std::fs::metadata(path).and_then(|m| m.modified()) else { return Ok(None) };
    if !fresh(&bytes, written) {
        return Ok(None);
    }
    match read(heap, target, &bytes) {
        Ok(templates) => Ok(Some(templates)),
        Err(e) if heap.libraries.pending_load.is_some() => Err(e),
        Err(_) => Ok(None),
    }
}

/// Write a cache file, best effort: whole, then renamed into place, so
/// another s2 never reads half of one.
pub fn write_cache(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path)).is_err() {
        std::fs::remove_file(&temp).ok();
    }
}

/// Load a compiled file into `target`: make its environments' bindings,
/// and return its templates, to be run in order. If a library it needs
/// isn't registered, the error leaves `pending_load` set, as the expander
/// does, for the caller to load the library and try again.
pub fn read(heap: &mut Heap, target: GcRef, bytes: &[u8]) -> Result<Vec<GcRef>, String> {
    // Not changes a program being compiled makes, if one is recording.
    let outer = env_of(target).borrow_mut().touched.take();
    let result = read_unit(heap, target, bytes);
    env_of(target).borrow_mut().touched = outer;
    result
}

fn read_unit(heap: &mut Heap, target: GcRef, bytes: &[u8]) -> Result<Vec<GcRef>, String> {
    let mut r = Reader { bytes, at: 0 };
    r.header()?;
    let needs = r.small::<usize>()?;
    for _ in 0..needs {
        let name = r.name()?;
        require_library(heap, &name)?;
    }
    let count: usize = r.small()?;
    let mut objects: Vec<Value> = Vec::with_capacity(count.min(1 << 20));
    // The environments whose cells may be made, not just found.
    let mut owned: FxHashSet<GcRef> = FxHashSet::default();
    owned.insert(target);
    let obj = |objects: &Vec<Value>, i: u64| objects.get(i as usize).copied().ok_or_else(|| TRUNCATED.to_string());
    let gc = |v: Value| v.as_ref().ok_or_else(|| TRUNCATED.to_string());
    for _ in 0..count {
        let v = match r.u8()? {
            PAIR => {
                let (a, d) = (val(&mut r, &objects)?, val(&mut r, &objects)?);
                heap.cons(a, d)
            }
            SYMBOL => heap.intern(&r.str()?),
            STRING => heap.string(&r.str()?),
            VECTOR => {
                let items = vals(&mut r, &objects)?;
                heap.vector(items)
            }
            BYTEVECTOR => {
                let n = r.small()?;
                let bytes = r.bytes(n)?.to_vec();
                heap.bytevector(bytes)
            }
            BIGINT => heap.integer(r.bigint()?),
            RATIONAL => {
                let (n, d) = (r.bigint()?, r.bigint()?);
                heap.rational(BigRational::new(n, d))
            }
            CELL => {
                let key = gc(obj(&objects, r.uint()?)?)?;
                let env = gc(obj(&objects, r.uint()?)?)?;
                let cell = env_of(env).borrow_mut().own_cell(heap, key, owned.contains(&env));
                let cell = cell.ok_or_else(|| format!("compiled file uses {}, which is no longer defined", print_value(Value::obj(key))))?;
                Value::obj(cell)
            }
            MACRO_REF => {
                let key = gc(obj(&objects, r.uint()?)?)?;
                let env = gc(obj(&objects, r.uint()?)?)?;
                let binding = env_of(env).borrow().lookup(key);
                let Some(TopBinding::Macro(m)) = binding else {
                    return Err(format!("compiled file uses the macro {}, which is no longer defined", print_value(Value::obj(key))));
                };
                Value::obj(m)
            }
            ALIAS => {
                let original = val(&mut r, &objects)?;
                let depth = r.small()?;
                let env = gc(obj(&objects, r.uint()?)?)?;
                heap.alias(original, depth, env)
            }
            ENVIRONMENT => Value::obj(match r.u8()? {
                TARGET => target,
                SYSTEM => heap.system.unwrap(),
                LIBRARY => {
                    let name = r.name()?;
                    require_library(heap, &name)?;
                    heap.libraries.get(&name).and_then(|l| l.env).ok_or(TRUNCATED)?
                }
                NEW => {
                    let env = heap.environment(Env::empty(r.u8()? != 0));
                    owned.insert(env);
                    env
                }
                _ => return Err(TRUNCATED.into()),
            }),
            MACRO => {
                let ellipsis = match r.uint()? {
                    0 => None,
                    i => Some(gc(obj(&objects, i - 1)?)?),
                };
                let n = r.small()?;
                let literals = (0..n).map(|_| gc(obj(&objects, r.uint()?)?)).collect::<Result<_, _>>()?;
                let flat = vals(&mut r, &objects)?;
                let rules = flat.chunks(2).map(|pt| (pt[0], pt[1])).collect();
                let depth = r.small()?;
                let env = gc(obj(&objects, r.uint()?)?)?;
                Value::obj(heap.make_macro(SyntaxRules { ellipsis, literals, rules, depth, env }))
            }
            TEMPLATE => {
                let name = r.str()?.into();
                let required = r.small()?;
                let rest = r.u8()? != 0;
                let nfree = r.small()?;
                let n = r.small()?;
                let code = (0..n).map(|_| r.op()).collect::<Result<_, _>>()?;
                let consts = vals(&mut r, &objects)?;
                let mut names = || -> Result<Vec<String>, String> {
                    let n = r.small()?;
                    (0..n).map(|_| r.str()).collect()
                };
                let (local_names, free_names) = (names()?, names()?);
                Value::obj(heap.template(Template { name, required, rest, nfree, code, consts, local_names, free_names }))
            }
            _ => return Err(TRUNCATED.into()),
        };
        objects.push(v);
    }
    // The environments' bindings.
    let groups: usize = r.small()?;
    for _ in 0..groups {
        let env = gc(obj(&objects, r.uint()?)?)?;
        if !owned.contains(&env) {
            return Err(TRUNCATED.into());
        }
        let n: usize = r.small()?;
        for _ in 0..n {
            let key = gc(obj(&objects, r.uint()?)?)?;
            let binding = match (r.u8()?, r.uint()?) {
                (0, i) => TopBinding::Variable(gc(obj(&objects, i)?)?),
                (1, i) => TopBinding::Macro(gc(obj(&objects, i)?)?),
                (2, i) => TopBinding::Core(CORE_FORMS.get(i as usize).ok_or(TRUNCATED)?.1),
                _ => return Err(TRUNCATED.into()),
            };
            let imported = r.u8()? != 0;
            env_of(env).borrow_mut().bind(key, binding, imported);
        }
    }
    let n: usize = r.small()?;
    (0..n).map(|_| gc(obj(&objects, r.uint()?)?)).collect()
}

fn val(r: &mut Reader, objects: &[Value]) -> Result<Value, String> {
    Ok(match r.u8()? {
        0 => Value::fixnum(r.int()?).ok_or(TRUNCATED)?,
        1 => Value::flonum(f64::from_le_bytes(r.bytes(8)?.try_into().unwrap())),
        2 => Value::char(char::from_u32(r.small()?).ok_or(TRUNCATED)?),
        3 => Value::TRUE,
        4 => Value::FALSE,
        5 => Value::NIL,
        6 => Value::UNSPECIFIED,
        7 => Value::EOF,
        8 => Value::UNDEFINED,
        9 => *objects.get(r.small::<usize>()?).ok_or(TRUNCATED)?,
        _ => return Err(TRUNCATED.into()),
    })
}

fn vals(r: &mut Reader, objects: &[Value]) -> Result<Vec<Value>, String> {
    let n: usize = r.small()?;
    (0..n).map(|_| val(r, objects)).collect()
}
