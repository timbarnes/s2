//! The virtual machine: instructions, templates and the dispatch loop.

pub mod machine;
pub mod op;
#[cfg(feature = "stats")]
pub mod stats;

pub use machine::{Frame, Vm};
pub use op::{Op, Template};
