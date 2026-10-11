//! Reading: characters to tokens to data on the heap.

pub mod number_syntax;
pub mod parser;
pub mod tokenizer;

pub use parser::{ParseError, parse};
