//! The virtual machine: instructions, templates and the dispatch loop.

pub mod machine;
pub mod op;

pub use machine::Vm;
pub use op::{Op, Template};
