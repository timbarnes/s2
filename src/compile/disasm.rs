//! The disassembler: a template as readable text (plan/03 §15).
//!
//! ```text
//! template top-level
//!    0  GlobalRef k0     ; <
//!    1  GlobalRef k1     ; x
//! ```

use crate::globals::cell_name;
use crate::printer::print_value;
use crate::vm::{Op, Template};

pub fn disassemble(t: &Template) -> String {
    let mut out = format!("template {}\n", t.name);
    for (i, op) in t.code.iter().enumerate() {
        let constant = |k: u32| t.consts[k as usize];
        let (text, note) = match *op {
            Op::Const(k) => (format!("Const k{}", k), Some(print_value(constant(k)))),
            Op::Fixnum(n) => (format!("Fixnum {}", n), None),
            Op::GlobalRef(k) => (format!("GlobalRef k{}", k), Some(cell(constant(k)))),
            Op::GlobalDefine(k) => (format!("GlobalDefine k{}", k), Some(cell(constant(k)))),
            Op::Jump(target) => (format!("Jump →{}", target), None),
            Op::JumpIfFalse(target) => (format!("JumpIfFalse →{}", target), None),
            Op::Call(n) => (format!("Call {}", n), None),
            op => (format!("{:?}", op), None),
        };
        match note {
            Some(note) => out.push_str(&format!("{:4}  {:<16} ; {}\n", i, text, note)),
            None => out.push_str(&format!("{:4}  {}\n", i, text)),
        }
    }
    out
}

fn cell(v: crate::value::Value) -> String {
    print_value(cell_name(v.as_ref().unwrap()))
}
