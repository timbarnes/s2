//! The disassembler: templates as readable text (plan/03 §15).
//!
//! ```text
//! template fib  arity 1
//!    0  GlobalRef k0     ; <
//!    1  LocalRef 0       ; n
//! ```
//!
//! The templates of inner lambdas follow the template that makes their
//! closures.

use crate::globals::cell_name;
use crate::heap::{GcRef, Obj};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::{Op, Template};

pub fn disassemble(template: GcRef) -> String {
    let mut out = String::new();
    template_into(&mut out, template);
    out
}

fn template_into(out: &mut String, template: GcRef) {
    let Obj::Template(t) = &template.get().kind else { unreachable!() };
    out.push_str(&header(t));
    let mut inner = Vec::new();
    for (i, op) in t.code.iter().enumerate() {
        let constant = |k: u32| t.consts[k as usize];
        let name = |names: &[String], i: u16| names.get(i as usize).cloned().unwrap_or_default();
        let (text, note) = match *op {
            Op::Const(k) => (format!("Const k{}", k), print_value(constant(k))),
            Op::Fixnum(n) => (format!("Fixnum {}", n), String::new()),
            Op::LocalRef(i) => (format!("LocalRef {}", i), name(&t.local_names, i)),
            Op::FreeRef(i) => (format!("FreeRef {}", i), name(&t.free_names, i)),
            Op::GlobalRef(k) => (format!("GlobalRef k{}", k), cell(constant(k))),
            Op::GlobalDefine(k) => (format!("GlobalDefine k{}", k), cell(constant(k))),
            Op::Jump(target) => (format!("Jump →{}", target), String::new()),
            Op::JumpIfFalse(target) => (format!("JumpIfFalse →{}", target), String::new()),
            Op::Slide(n) => (format!("Slide {}", n), String::new()),
            Op::MakeClosure(k) => {
                let r = constant(k).as_ref().unwrap();
                inner.push(r);
                (format!("MakeClosure k{}", k), print_value(constant(k)))
            }
            Op::Call(n) => (format!("Call {}", n), String::new()),
            Op::TailCall(n) => (format!("TailCall {}", n), String::new()),
            op => (format!("{:?}", op), String::new()),
        };
        if note.is_empty() {
            out.push_str(&format!("{:4}  {}\n", i, text));
        } else {
            out.push_str(&format!("{:4}  {:<16} ; {}\n", i, text, note));
        }
    }
    for r in inner {
        template_into(out, r);
    }
}

fn header(t: &Template) -> String {
    let name = if t.name.is_empty() { "(lambda)" } else { &t.name };
    let mut h = format!("template {}", name);
    if &*t.name != "top-level" {
        h.push_str(&format!("  arity {}{}", t.required, if t.rest { "+" } else { "" }));
    }
    if t.nfree > 0 {
        h.push_str(&format!("  free {}", t.nfree));
    }
    h.push('\n');
    h
}

fn cell(v: Value) -> String {
    print_value(cell_name(v.as_ref().unwrap()))
}
