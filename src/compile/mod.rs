//! The compiler: datum → core AST → template (plan/03 §1).

pub mod ast;
pub mod codegen;
pub mod disasm;
pub mod expand;

use crate::globals::Env;
use crate::heap::Heap;
use crate::value::Value;
use crate::vm::Template;

/// Compile a top-level form, for the environment `env`.
pub fn compile(heap: &mut Heap, env: &mut Env, datum: Value) -> Result<Template, String> {
    let ast = expand::expand(heap, env, datum)?;
    Ok(codegen::codegen(&ast))
}
