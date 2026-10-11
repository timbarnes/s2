//! The dispatch loop (D4): a stack machine.
//!
//! P1 has no frames: a template runs from its first instruction to its
//! `Return`, and calls go only to primitives, which return at once. Frames
//! arrive with `lambda` in P2.

use crate::builtins::PrimFn;
use crate::globals::{Env, cell_name, cell_value};
use crate::heap::{GcRef, Heap, Obj};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::op::{Op, Template};

/// Run a template of no arguments and return its value. An error aborts the
/// whole form; there is no `raise` until P7.
/// `env` is the environment the code was compiled for, which VM primitives
/// such as `dis` need.
pub fn run(heap: &mut Heap, env: &mut Env, t: &Template) -> Result<Value, String> {
    let mut stack: Vec<Value> = Vec::new();
    let mut ip = 0;
    loop {
        let op = t.code[ip];
        ip += 1;
        match op {
            Op::Const(k) => stack.push(t.consts[k as usize]),
            Op::Fixnum(n) => stack.push(Value::fixnum(n as i64).unwrap()),
            Op::Nil => stack.push(Value::NIL),
            Op::True => stack.push(Value::TRUE),
            Op::False => stack.push(Value::FALSE),
            Op::Unspecified => stack.push(Value::UNSPECIFIED),
            Op::GlobalRef(k) => {
                let cell = cell(t, k);
                let v = cell_value(cell).get();
                if v.is_undefined() {
                    return Err(format!("unbound variable: {}", print_value(cell_name(cell))));
                }
                stack.push(v);
            }
            Op::GlobalDefine(k) => {
                let cell = cell(t, k);
                cell_value(cell).set(stack.pop().unwrap());
                stack.push(cell_name(cell));
            }
            Op::Jump(target) => ip = target as usize,
            Op::JumpIfFalse(target) => {
                if !stack.pop().unwrap().is_true() {
                    ip = target as usize;
                }
            }
            Op::Pop => {
                stack.pop();
            }
            Op::Call(argc) => {
                let base = stack.len() - argc as usize - 1;
                let f = stack[base];
                let Some(Obj::Primitive(p)) = f.as_obj().map(|o| &o.kind) else {
                    return Err(format!("not a procedure: {}", print_value(f)));
                };
                let n = argc as usize;
                if n < p.min || p.max.is_some_and(|max| n > max) {
                    return Err(format!("{}: wrong number of arguments: {}", p.name, n));
                }
                let args = &stack[base + 1..];
                let result = match p.f {
                    PrimFn::Builtin(f) => f(heap, args)?,
                    PrimFn::Vm(f) => f(heap, env, args)?,
                };
                stack.truncate(base);
                stack.push(result);
            }
            Op::Return => return Ok(stack.pop().unwrap()),
        }
    }
}

fn cell(t: &Template, k: u32) -> GcRef {
    t.consts[k as usize].as_ref().unwrap()
}
