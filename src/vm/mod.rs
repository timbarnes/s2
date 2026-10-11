//! The virtual machine: instructions, templates and the dispatch loop.

pub mod machine;
pub mod op;

pub use machine::run;
pub use op::{Op, Template};
