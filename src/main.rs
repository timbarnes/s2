//! s2: a bytecode-compiled Scheme. See plan/ for the design.
//!
//! P0: read a datum, write it back. Usage:
//!
//! ```text
//! s2            read-print loop on standard input
//! s2 -e TEXT    read each datum in TEXT and write it, one per line
//! ```

// ponytail: the forked and foundation modules carry API that later phases
// use (ports in P6, the GC mark in P3, more value accessors in P1).
// Remove these allows as the callers arrive.
#[allow(dead_code)]
mod heap;
#[allow(dead_code)]
mod io;
#[allow(dead_code)]
mod printer;
mod reader;
#[allow(dead_code)]
mod sstring;
#[allow(dead_code)]
mod value;

use std::io::{IsTerminal, Write};

use heap::Heap;
use io::PortKind;
use reader::{ParseError, parse};

/// Read every datum from `port` and write each one, one per line. Returns
/// false if any datum had a syntax error.
fn read_print(heap: &mut Heap, port: &mut PortKind, prompt: bool) -> bool {
    let mut ok = true;
    loop {
        if prompt {
            print!("s2> ");
            std::io::stdout().flush().ok();
        }
        match parse(heap, port) {
            Ok(v) => println!("{}", printer::print_value(v)),
            Err(ParseError::Eof) => return ok,
            Err(ParseError::Syntax(msg)) => {
                eprintln!("error: {}", msg);
                ok = false;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut heap = Heap::new();
    let ok = match args.as_slice() {
        [] => {
            let prompt = std::io::stdin().is_terminal();
            let ok = read_print(&mut heap, &mut PortKind::Stdin, prompt);
            if prompt {
                println!();
            }
            ok
        }
        [flag, text] if flag == "-e" => read_print(&mut heap, &mut io::new_string_port_input(text), false),
        _ => {
            eprintln!("usage: s2 [-e TEXT]");
            std::process::exit(2);
        }
    };
    std::process::exit(if ok { 0 } else { 1 });
}
