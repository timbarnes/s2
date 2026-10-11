//! The compiler: datum → core AST → template (plan/03 §1).

pub mod analyze;
pub mod ast;
pub mod codegen;
pub mod disasm;
pub mod expand;
pub mod syntax_rules;

use crate::globals::Env;
use crate::heap::{GcRef, Heap};
use crate::value::Value;

/// Compile a top-level form, for the environment `env`, into a template
/// of no arguments.
pub fn compile(heap: &mut Heap, env: &mut Env, datum: Value) -> Result<GcRef, String> {
    let ast = expand::expand(heap, env, datum)?;
    Ok(codegen::codegen(heap, &ast))
}
