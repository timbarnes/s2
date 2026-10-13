//! The compiler's data as Scheme values (plan/08, C1 and C2), for the
//! compiler in Scheme: the core AST as tagged vectors, and templates as
//! their fields, with instructions as lists.
//!
//! **The AST.** Each node is a vector whose first element is its tag:
//!
//! ```text
//! #(const v)              #(local-ref id)          #(local-set id e)
//! #(global-ref cell)      #(global-set cell e)     #(global-define cell e)
//! #(if test then else)    #(seq (e ...))           #(call f (arg ...))
//! #(lambda name (var ...) rest body)               name a string, "" if none;
//!                                                  rest a var, or #f
//! #(let ((var . init) ...) body)                   #(letrec ((var . init) ...) body)
//! #(var id name)          a binding occurrence: its number, and its name (a symbol)
//! ```
//!
//! **Instructions** are lists, `(name operand ...)`, named as in `INSTRUCTIONS`
//! below; a `JumpIfNot`'s test is the procedure's name (`null?`, `<`, ...).
//! `%make-template` builds a `Template` from them, checking every operand it
//! can: constant indexes, the kind of constant each instruction needs, jump
//! targets and free-variable indexes. It can't check stack slots, which
//! depend on the stack depth at each instruction.

use crate::builtins::{Error, Primitive, err, kind, prim};
use crate::compile::ast::{Expr, Lambda, Var};
use crate::compile::syntax_rules::name as id_name;
use crate::heap::{GcRef, Heap, Obj, list_to_vec};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::op::Test;
use crate::vm::{Op, Template};

// ---------------------------------------------------------------------------
// The AST

fn tagged(heap: &mut Heap, tag: &str, fields: Vec<Value>) -> Value {
    let mut items = vec![heap.intern(tag)];
    items.extend(fields);
    heap.vector(items)
}

fn var_datum(heap: &mut Heap, v: &Var) -> Value {
    let id = Value::fixnum(v.id as i64).unwrap();
    tagged(heap, "var", vec![id, v.name])
}

fn bindings_datum(heap: &mut Heap, bindings: &[(Var, Expr)]) -> Value {
    let pairs = bindings
        .iter()
        .map(|(v, init)| {
            let (v, init) = (var_datum(heap, v), expr_to_datum(heap, init));
            heap.cons(v, init)
        })
        .collect();
    heap.list(pairs)
}

/// The AST `e` as tagged vectors.
pub fn expr_to_datum(heap: &mut Heap, e: &Expr) -> Value {
    let id = |id: u32| Value::fixnum(id as i64).unwrap();
    match e {
        Expr::Const(v) => tagged(heap, "const", vec![*v]),
        Expr::LocalRef(v) => tagged(heap, "local-ref", vec![id(*v)]),
        Expr::LocalSet(v, x) => {
            let x = expr_to_datum(heap, x);
            tagged(heap, "local-set", vec![id(*v), x])
        }
        Expr::GlobalRef(c) => tagged(heap, "global-ref", vec![Value::obj(*c)]),
        Expr::GlobalSet(c, x) | Expr::GlobalDefine(c, x) => {
            let tag = if matches!(e, Expr::GlobalSet(..)) { "global-set" } else { "global-define" };
            let x = expr_to_datum(heap, x);
            tagged(heap, tag, vec![Value::obj(*c), x])
        }
        Expr::If(a, b, c) => {
            let parts = [a, b, c].map(|x| expr_to_datum(heap, x)).to_vec();
            tagged(heap, "if", parts)
        }
        Expr::Seq(xs) => {
            let items = xs.iter().map(|x| expr_to_datum(heap, x)).collect();
            let list = heap.list(items);
            tagged(heap, "seq", vec![list])
        }
        Expr::Lambda(l) => {
            let name = heap.string(&l.name);
            let params = l.params.iter().map(|p| var_datum(heap, p)).collect();
            let params = heap.list(params);
            let rest = match &l.rest {
                Some(r) => var_datum(heap, r),
                None => Value::FALSE,
            };
            let body = expr_to_datum(heap, &l.body);
            tagged(heap, "lambda", vec![name, params, rest, body])
        }
        Expr::Call(f, args) => {
            let f = expr_to_datum(heap, f);
            let args = args.iter().map(|a| expr_to_datum(heap, a)).collect();
            let args = heap.list(args);
            tagged(heap, "call", vec![f, args])
        }
        Expr::Let(bindings, body) | Expr::Letrec(bindings, body) => {
            let tag = if matches!(e, Expr::Let(..)) { "let" } else { "letrec" };
            let bindings = bindings_datum(heap, bindings);
            let body = expr_to_datum(heap, body);
            tagged(heap, tag, vec![bindings, body])
        }
    }
}

/// The fields of the node `d`, after its tag, and the tag.
fn node(d: Value) -> Result<(&'static str, Vec<Value>), String> {
    let bad = || format!("not an AST node: {}", print_value(d));
    let Some(Obj::Vector(items)) = kind(d) else { return Err(bad()) };
    let items: Vec<Value> = items.iter().map(|c| c.get()).collect();
    let tag = items.first().and_then(|t| t.as_ref()).filter(|t| matches!(t.get().kind, Obj::Symbol(_))).ok_or_else(bad)?;
    // Symbols are never freed while a datum refers to them, and the tag's
    // name is only compared, not kept.
    let name: &'static str = id_name(tag);
    Ok((name, items[1..].to_vec()))
}

fn var_id(v: Value) -> Result<u32, String> {
    v.as_fixnum().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("not a variable number: {}", print_value(v)))
}

fn cell(v: Value) -> Result<GcRef, String> {
    match kind(v) {
        Some(Obj::Global { .. }) => Ok(v.as_ref().unwrap()),
        _ => Err(format!("not a global variable's cell: {}", print_value(v))),
    }
}

fn datum_to_var(d: Value) -> Result<Var, String> {
    match node(d)? {
        ("var", f) if f.len() == 2 => Ok(Var { id: var_id(f[0])?, name: f[1] }),
        _ => Err(format!("not a variable: {}", print_value(d))),
    }
}

fn items(d: Value) -> Result<Vec<Value>, String> {
    list_to_vec(d).ok_or_else(|| format!("not a list: {}", print_value(d)))
}

fn datum_to_bindings(d: Value) -> Result<Vec<(Var, Expr)>, String> {
    items(d)?
        .into_iter()
        .map(|pair| match kind(pair) {
            Some(Obj::Pair { car, cdr }) => Ok((datum_to_var(car.get())?, datum_to_expr(cdr.get())?)),
            _ => Err(format!("not a binding: {}", print_value(pair))),
        })
        .collect()
}

/// The AST that the tagged vectors `d` stand for.
pub fn datum_to_expr(d: Value) -> Result<Expr, String> {
    let sub = |x: Value| datum_to_expr(x).map(Box::new);
    let (tag, f) = node(d)?;
    Ok(match (tag, f.as_slice()) {
        ("const", &[v]) => Expr::Const(v),
        ("local-ref", &[v]) => Expr::LocalRef(var_id(v)?),
        ("local-set", &[v, x]) => Expr::LocalSet(var_id(v)?, sub(x)?),
        ("global-ref", &[c]) => Expr::GlobalRef(cell(c)?),
        ("global-set", &[c, x]) => Expr::GlobalSet(cell(c)?, sub(x)?),
        ("global-define", &[c, x]) => Expr::GlobalDefine(cell(c)?, sub(x)?),
        ("if", &[a, b, c]) => Expr::If(sub(a)?, sub(b)?, sub(c)?),
        ("seq", &[xs]) => Expr::Seq(items(xs)?.into_iter().map(datum_to_expr).collect::<Result<_, _>>()?),
        ("call", &[f, args]) => Expr::Call(sub(f)?, items(args)?.into_iter().map(datum_to_expr).collect::<Result<_, _>>()?),
        ("lambda", &[name, params, rest, body]) => {
            let Some(Obj::Str(name)) = kind(name) else { return Err(format!("not a procedure name: {}", print_value(name))) };
            Expr::Lambda(Box::new(Lambda {
                name: name.borrow().to_string().into(),
                params: items(params)?.into_iter().map(datum_to_var).collect::<Result<_, _>>()?,
                rest: if rest.is_true() { Some(datum_to_var(rest)?) } else { None },
                body: datum_to_expr(body)?,
            }))
        }
        ("let", &[bindings, body]) => Expr::Let(datum_to_bindings(bindings)?, sub(body)?),
        ("letrec", &[bindings, body]) => Expr::Letrec(datum_to_bindings(bindings)?, sub(body)?),
        _ => return Err(format!("not an AST node: {}", print_value(d))),
    })
}

// ---------------------------------------------------------------------------
// Instructions

/// An instruction's operand, as data.
trait Operand: Sized {
    fn to_datum(self, heap: &mut Heap) -> Value;
    fn from_datum(d: Value) -> Option<Self>;
}

macro_rules! integer_operand {
    ($($t:ty),*) => {$(
        impl Operand for $t {
            fn to_datum(self, _: &mut Heap) -> Value {
                Value::fixnum(self as i64).unwrap()
            }
            fn from_datum(d: Value) -> Option<$t> {
                d.as_fixnum().and_then(|n| <$t>::try_from(n).ok())
            }
        }
    )*};
}
integer_operand!(u16, u32, i16, i32);

const TESTS: [(Test, &str); 8] = [
    (Test::NullP, "null?"),
    (Test::PairP, "pair?"),
    (Test::EqP, "eq?"),
    (Test::NumEq, "="),
    (Test::Lt, "<"),
    (Test::Gt, ">"),
    (Test::Le, "<="),
    (Test::Ge, ">="),
];

impl Operand for Test {
    fn to_datum(self, heap: &mut Heap) -> Value {
        heap.intern(TESTS.iter().find(|(t, _)| *t == self).unwrap().1)
    }
    fn from_datum(d: Value) -> Option<Test> {
        let name = d.as_ref().filter(|s| matches!(s.get().kind, Obj::Symbol(_))).map(id_name)?;
        TESTS.iter().find(|(_, n)| *n == name).map(|(t, _)| *t)
    }
}

/// Every instruction's name and operands, in both directions.
macro_rules! instructions {
    ($($variant:ident $name:literal $(($($arg:ident),*))?;)*) => {
        /// The instruction `op` as data.
        pub fn op_to_datum(heap: &mut Heap, op: Op) -> Value {
            match op {
                $(Op::$variant $(($($arg),*))? => {
                    let parts = vec![heap.intern($name) $($(, $arg.to_datum(heap))*)?];
                    heap.list(parts)
                })*
            }
        }

        /// The instruction the data `d` stands for.
        pub fn datum_to_op(d: Value) -> Result<Op, String> {
            let bad = || format!("not an instruction: {}", print_value(d));
            let parts = list_to_vec(d).ok_or_else(bad)?;
            let name = parts.first().and_then(|p| p.as_ref()).filter(|s| matches!(s.get().kind, Obj::Symbol(_))).map(id_name).ok_or_else(bad)?;
            #[allow(unused_variables)]
            let args = &parts[1..];
            match name {
                $($name => match args {
                    [$($($arg),*)?] => Ok(Op::$variant $(($(Operand::from_datum(*$arg).ok_or_else(bad)?),*))?),
                    _ => Err(bad()),
                },)*
                _ => Err(bad()),
            }
        }
    };
}

instructions! {
    Const "const" (k);
    Fixnum "fixnum" (n);
    Nil "nil";
    True "true";
    False "false";
    Unspecified "unspecified";
    Undefined "undefined";
    LocalRef "local-ref" (i);
    LocalSet "local-set" (i);
    FreeRef "free-ref" (i);
    PatchFree "patch-free" (c, i, v);
    MakeBox "make-box" (i);
    BoxRef "box-ref";
    BoxSet "box-set";
    GlobalRef "global-ref" (k);
    GlobalSet "global-set" (k);
    GlobalDefine "global-define" (k);
    Jump "jump" (t);
    JumpIfFalse "jump-if-false" (t);
    JumpIfNot "jump-if-not" (test, t);
    Pop "pop";
    Slide "slide" (n);
    MakeClosure "make-closure" (k);
    Call "call" (n);
    TailCall "tail-call" (n);
    Return "return";
    SelfTailCall "self-tail-call" (n);
    Car "car";
    Cdr "cdr";
    Cons "cons";
    NullP "null?";
    PairP "pair?";
    EqP "eq?";
    Not "not";
    Add "add";
    Sub "sub";
    Mul "mul";
    Div "div";
    NumEq "num-eq";
    Lt "lt";
    Gt "gt";
    Le "le";
    Ge "ge";
    VectorLength "vector-length";
    VectorRef "vector-ref";
    VectorSet "vector-set";
    CarLocal "car-local" (i);
    CdrLocal "cdr-local" (i);
    AddImm "add-imm" (k);
    SubImm "sub-imm" (k);
    JumpIfNotImm "jump-if-not-imm" (test, k, t);
}

// ---------------------------------------------------------------------------
// Templates

/// The fields `%make-template` takes, in order.
pub struct TemplateFields {
    pub name: String,
    pub required: u16,
    pub rest: bool,
    pub nfree: u16,
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    pub local_names: Vec<String>,
    pub free_names: Vec<String>,
}

/// Check `f`'s instructions against its constants and free variables, and
/// make the template.
pub fn make_template(heap: &mut Heap, f: TemplateFields) -> Result<GcRef, String> {
    let who = |op: Op, why: &str| format!("%make-template: {:?} in {}: {}", op, f.name, why);
    for &op in &f.code {
        let constant = |k: u32| f.consts.get(k as usize).copied().ok_or_else(|| who(op, "no such constant"));
        let target = |t: u32| if (t as usize) < f.code.len() { Ok(()) } else { Err(who(op, "jump past the end")) };
        match op {
            Op::Const(k) => constant(k).map(|_| ())?,
            Op::GlobalRef(k) | Op::GlobalSet(k) | Op::GlobalDefine(k) => {
                if !matches!(kind(constant(k)?), Some(Obj::Global { .. })) {
                    return Err(who(op, "not a global variable's cell"));
                }
            }
            Op::MakeClosure(k) => {
                if !matches!(kind(constant(k)?), Some(Obj::Template(_))) {
                    return Err(who(op, "not a template"));
                }
            }
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfNot(_, t) | Op::JumpIfNotImm(_, _, t) => target(t)?,
            Op::FreeRef(i) if i >= f.nfree => return Err(who(op, "no such free variable")),
            _ => {}
        }
    }
    if f.free_names.len() != f.nfree as usize {
        return Err(format!("%make-template: {}: {} free variables, but {} names", f.name, f.nfree, f.free_names.len()));
    }
    Ok(heap.template(Template {
        name: f.name.into(),
        required: f.required,
        rest: f.rest,
        nfree: f.nfree,
        code: f.code,
        consts: f.consts,
        local_names: f.local_names,
        free_names: f.free_names,
    }))
}

/// A template's fields as data, the arguments `%make-template` takes, with
/// its inner templates (constants) also taken apart and remade, for the
/// round trip check.
pub fn remake(heap: &mut Heap, t: GcRef) -> Result<GcRef, String> {
    let Obj::Template(t) = &t.get().kind else { unreachable!("not a template") };
    let mut consts = Vec::with_capacity(t.consts.len());
    for &c in &t.consts {
        consts.push(match kind(c) {
            Some(Obj::Template(_)) => Value::obj(remake(heap, c.as_ref().unwrap())?),
            _ => c,
        });
    }
    let strings = |heap: &mut Heap, names: &[String]| {
        let items = names.iter().map(|n| heap.string(n)).collect();
        heap.vector(items)
    };
    let code: Vec<Value> = t.code.iter().map(|&op| op_to_datum(heap, op)).collect();
    let args = [
        heap.string(&t.name),
        Value::fixnum(t.required as i64).unwrap(),
        Value::bool(t.rest),
        Value::fixnum(t.nfree as i64).unwrap(),
        heap.vector(code),
        heap.vector(consts),
        strings(heap, &t.local_names),
        strings(heap, &t.free_names),
    ];
    let fields = template_fields(&args).map_err(|e| e.message)?;
    make_template(heap, fields)
}

fn vector_items(v: Value, what: &str) -> Result<Vec<Value>, Error> {
    match kind(v) {
        Some(Obj::Vector(items)) => Ok(items.iter().map(|c| c.get()).collect()),
        _ => Err(err!("%make-template: {} must be a vector: {}", what, print_value(v))),
    }
}

fn string_value(v: Value, what: &str) -> Result<String, Error> {
    match kind(v) {
        Some(Obj::Str(s)) => Ok(s.borrow().to_string()),
        _ => Err(err!("%make-template: {} must be a string: {}", what, print_value(v))),
    }
}

fn template_fields(a: &[Value]) -> Result<TemplateFields, Error> {
    let count = |v: Value, what: &str| v.as_fixnum().and_then(|n| u16::try_from(n).ok()).ok_or_else(|| err!("%make-template: bad {}: {}", what, print_value(v)));
    let names = |v: Value, what: &str| vector_items(v, what)?.into_iter().map(|n| string_value(n, what)).collect::<Result<Vec<_>, _>>();
    Ok(TemplateFields {
        name: string_value(a[0], "the name")?,
        required: count(a[1], "number of required parameters")?,
        rest: a[2].is_true(),
        nfree: count(a[3], "number of free variables")?,
        code: vector_items(a[4], "the code")?.into_iter().map(datum_to_op).collect::<Result<_, _>>()?,
        consts: vector_items(a[5], "the constants")?,
        local_names: names(a[6], "the local names")?,
        free_names: names(a[7], "the free-variable names")?,
    })
}

pub static PRIMITIVES: &[Primitive] = &[
    // (%make-template name required rest? nfree #(instruction ...) #(constant ...)
    //                 #(local-name ...) #(free-name ...))
    prim("%make-template", 8, Some(8), |h, a| {
        let fields = template_fields(a)?;
        Ok(Value::obj(make_template(h, fields)?))
    }),
    // (%inline-instruction cell argc): the instruction a call to the global
    // variable `cell` with `argc` arguments is inlined as, or #f.
    prim("%inline-instruction", 2, Some(2), |h, a| {
        let cell = cell(a[0]).map_err(Error::from)?;
        let argc = a[1].as_fixnum().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| err!("%inline-instruction: bad argument count: {}", print_value(a[1])))?;
        Ok(match crate::compile::codegen::inline_op(h, cell, argc) {
            Some(op) => op_to_datum(h, op),
            None => Value::FALSE,
        })
    }),
    // (%template->procedure t): a top-level form's template, as a procedure
    // of no arguments that runs it.
    prim("%template->procedure", 1, Some(1), |h, a| match kind(a[0]) {
        Some(Obj::Template(_)) => Ok(h.closure(a[0].as_ref().unwrap(), std::iter::empty())),
        _ => Err(err!("%template->procedure: not a template: {}", print_value(a[0]))),
    }),
    // The inverse, for checking: (%template-instructions t) is t's code as data.
    prim("%template-instructions", 1, Some(1), |h, a| {
        let Some(Obj::Template(t)) = kind(a[0]) else { return Err(err!("%template-instructions: not a template: {}", print_value(a[0]))) };
        let code: Vec<Op> = t.code.clone();
        let items = code.into_iter().map(|op| op_to_datum(h, op)).collect();
        Ok(h.list(items))
    }),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_instruction_round_trips() {
        let mut heap = Heap::new();
        let ops = [
            Op::Const(3), Op::Fixnum(-7), Op::Nil, Op::True, Op::False, Op::Unspecified, Op::Undefined,
            Op::LocalRef(1), Op::LocalSet(2), Op::FreeRef(0), Op::PatchFree(1, 2, 3), Op::MakeBox(4),
            Op::BoxRef, Op::BoxSet, Op::GlobalRef(5), Op::GlobalSet(6), Op::GlobalDefine(7), Op::Jump(8),
            Op::JumpIfFalse(9), Op::JumpIfNot(Test::Le, 10), Op::Pop, Op::Slide(2), Op::MakeClosure(1),
            Op::Call(2), Op::TailCall(3), Op::Return, Op::SelfTailCall(1), Op::Car, Op::Cdr, Op::Cons,
            Op::NullP, Op::PairP, Op::EqP, Op::Not, Op::Add, Op::Sub, Op::Mul, Op::Div, Op::NumEq, Op::Lt,
            Op::Gt, Op::Le, Op::Ge, Op::VectorLength, Op::VectorRef, Op::VectorSet, Op::CarLocal(1),
            Op::CdrLocal(2), Op::AddImm(-1), Op::SubImm(2), Op::JumpIfNotImm(Test::NumEq, -3, 4),
        ];
        for op in ops {
            let d = op_to_datum(&mut heap, op);
            assert_eq!(datum_to_op(d), Ok(op), "{}", print_value(d));
        }
        for t in TESTS {
            let d = Test::to_datum(t.0, &mut heap);
            assert_eq!(Test::from_datum(d), Some(t.0));
        }
        let name = heap.intern("local-ref");
        let bad = heap.list(vec![name]);
        assert!(datum_to_op(bad).is_err());
    }
}
