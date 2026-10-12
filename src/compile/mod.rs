//! The compiler: datum → core AST → template (plan/03 §1).

pub mod analyze;
pub mod ast;
pub mod codegen;
pub mod data;
pub mod disasm;
pub mod expand;
pub mod syntax_rules;

use crate::heap::{GcRef, Heap};
use crate::value::Value;

/// Compile a top-level form, for the environment `env` (an
/// `Obj::Environment`), into a template of no arguments.
pub fn compile(heap: &mut Heap, env: GcRef, datum: Value) -> Result<GcRef, String> {
    let ast = expand::expand(heap, env, datum)?;
    Ok(generate(heap, &ast))
}

/// Which compiler `S2_COMPILER` selects (plan/08, C7):
///
/// - `rust`, the default;
/// - `roundtrip`: the Rust compiler, with its AST and templates passed
///   through their forms as Scheme data and back, to check those
///   conversions;
/// - `scheme`: the code generator in Scheme (`scheme/compiler/`), where
///   the VM can call it: the REPL, `-e`, `-f`, programs and `--compile`
///   (main.rs). Library files and `eval` compile inside a primitive, where
///   the VM can't be entered, so they keep the Rust compiler until every
///   compile is driven from Scheme (P13.4).
#[derive(Clone, Copy, PartialEq)]
pub enum Compiler {
    Rust,
    RoundTrip,
    Scheme,
}

pub fn compiler() -> Compiler {
    static CHOICE: std::sync::OnceLock<Compiler> = std::sync::OnceLock::new();
    *CHOICE.get_or_init(|| match std::env::var("S2_COMPILER").as_deref() {
        Err(_) | Ok("rust") => Compiler::Rust,
        Ok("roundtrip") => Compiler::RoundTrip,
        Ok("scheme") => Compiler::Scheme,
        Ok(other) => {
            eprintln!("s2: S2_COMPILER must be rust, roundtrip or scheme, not {}", other);
            std::process::exit(2)
        }
    })
}

/// Generate the code for a top-level form's AST, in Rust (for `scheme`,
/// where the Scheme compiler can't be called).
pub fn generate(heap: &mut Heap, ast: &ast::Expr) -> GcRef {
    match compiler() {
        Compiler::Rust | Compiler::Scheme => codegen::codegen(heap, ast),
        Compiler::RoundTrip => {
            let datum = data::expr_to_datum(heap, ast);
            let ast = data::datum_to_expr(datum).unwrap_or_else(|e| panic!("AST round trip: {}", e));
            let t = codegen::codegen(heap, &ast);
            data::remake(heap, t).unwrap_or_else(|e| panic!("template round trip: {}", e))
        }
    }
}
