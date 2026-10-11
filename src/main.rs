//! s2: a bytecode-compiled Scheme. See plan/ for the design.
//!
//! Usage:
//!
//! ```text
//! s2 [--dis]            REPL on standard input
//! s2 [--dis] -e TEXT    evaluate each form in TEXT, writing each result
//! s2 [--dis] FILE       run a program; results are not written
//! ```
//!
//! `--dis` prints each form's compiled code before running it.

mod builtins;
mod compile;
mod globals;
// ponytail: the forked and foundation modules carry API that later phases
// use (ports in P6, the GC mark in P3). Remove these allows as the callers
// arrive.
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
mod vm;

use std::io::{IsTerminal, Write};

use globals::Env;
use heap::Heap;
use io::PortKind;
use reader::{ParseError, parse};
use value::Value;

/// One top-level environment and its heap.
struct Session {
    heap: Heap,
    env: Env,
    dis: bool,
}

impl Session {
    fn new(dis: bool) -> Session {
        let mut heap = Heap::new();
        let mut env = Env::default();
        builtins::install(&mut heap, &mut env);
        Session { heap, env, dis }
    }

    /// Compile one top-level form and run it.
    fn eval(&mut self, datum: Value) -> Result<Value, String> {
        let template = compile::compile(&mut self.heap, &mut self.env, datum)?;
        if self.dis {
            print!("{}", compile::disasm::disassemble(&template));
        }
        vm::run(&mut self.heap, &template)
    }

    /// Evaluate every form from `port`. Returns false if any form had an
    /// error; an error is reported and evaluation continues with the next
    /// form.
    fn eval_port(&mut self, port: &mut PortKind, prompt: bool, write_results: bool) -> bool {
        let mut ok = true;
        loop {
            if prompt {
                print!("s2> ");
                std::io::stdout().flush().ok();
            }
            let result = match parse(&mut self.heap, port) {
                Ok(datum) => self.eval(datum),
                Err(ParseError::Eof) => return ok,
                Err(ParseError::Syntax(msg)) => Err(msg),
            };
            match result {
                Ok(v) if write_results && !v.is_unspecified() => println!("{}", printer::print_value(v)),
                Ok(_) => {}
                Err(msg) => {
                    std::io::stdout().flush().ok();
                    eprintln!("error: {}", msg);
                    ok = false;
                }
            }
        }
    }
}

fn usage() -> ! {
    eprintln!("usage: s2 [--dis] [-e TEXT | FILE]");
    std::process::exit(2);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dis = args.first().is_some_and(|a| a == "--dis");
    if dis {
        args.remove(0);
    }
    let mut session = Session::new(dis);
    let ok = match args.as_slice() {
        [] => {
            let prompt = std::io::stdin().is_terminal();
            let ok = session.eval_port(&mut PortKind::Stdin, prompt, true);
            if prompt {
                println!();
            }
            ok
        }
        [flag, text] if flag == "-e" => session.eval_port(&mut io::new_string_port_input(text), false, true),
        [file] if !file.starts_with('-') => match std::fs::read_to_string(file) {
            Ok(text) => session.eval_port(&mut io::new_string_port_input(&text), false, false),
            Err(e) => {
                eprintln!("s2: {}: {}", file, e);
                false
            }
        },
        _ => usage(),
    };
    std::io::stdout().flush().ok();
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evaluate each form in `src` and write each result, or `error: msg`.
    fn run(src: &str) -> Vec<String> {
        let mut s = Session::new(false);
        let mut port = io::new_string_port_input(src);
        let mut out = Vec::new();
        loop {
            match parse(&mut s.heap, &mut port) {
                Ok(datum) => out.push(match s.eval(datum) {
                    Ok(v) => printer::print_value(v),
                    Err(msg) => format!("error: {}", msg),
                }),
                Err(ParseError::Eof) => return out,
                Err(ParseError::Syntax(msg)) => panic!("{}", msg),
            }
        }
    }

    fn check(src: &str, expected: &[&str]) {
        assert_eq!(run(src), expected, "{}", src);
    }

    #[test]
    fn constants() {
        check("42 -7 #t #f '() \"s\" #\\a 1.5 'sym '(1 2) #(1 2) 12345678901234567890",
              &["42", "-7", "#t", "#f", "()", "\"s\"", "#\\a", "1.5", "sym", "(1 2)", "#(1 2)", "12345678901234567890"]);
    }

    #[test]
    fn arithmetic() {
        check("(+) (+ 1 2 3) (- 5) (- 10 1 2) (* 6 7) (*) (< 1 2 3) (< 1 3 2) (= 2 2) (> 3 2 1)",
              &["0", "6", "-5", "7", "42", "1", "#t", "#f", "#t", "#t"]);
    }

    #[test]
    fn overflow_makes_bignums() {
        check("(+ 1152921504606846975 1) (* 1152921504606846975 4) (- -1152921504606846976 1)",
              &["1152921504606846976", "4611686018427387900", "-1152921504606846977"]);
    }

    #[test]
    fn nested_calls() {
        check("(+ (* 2 3) (- 10 (* 2 2)))", &["12"]);
        check("(car (cdr (cons 1 (cons 2 '()))))", &["2"]);
    }

    #[test]
    fn if_and_truth() {
        // Only #f is false.
        check("(if #t 1 2) (if #f 1 2) (if 0 1 2) (if '() 1 2) (if (< 1 2) 'yes 'no) (if #f #f)",
              &["1", "2", "1", "1", "yes", ""]);
        check("(if (if #f #t #f) 1 (if #t 2 3))", &["2"]);
    }

    #[test]
    fn define_and_late_binding() {
        check("(define x 10) x (if (< x 20) (+ x 1) 0) (define x 'new) x",
              &["x", "10", "11", "x", "new"]);
        // The whole `begin` is compiled before `a` is defined; `a`'s cell is
        // created unbound at compile time and filled when the code runs.
        check("(begin (define a 1) (define b (+ a 1)) b)", &["2"]);
    }

    #[test]
    fn errors_abort_the_form_only() {
        check("y (define y 5) y", &["error: unbound variable: y", "y", "5"]);
        check("(car 1)", &["error: car: not a pair: 1"]);
        check("(1 2)", &["error: not a procedure: 1"]);
        check("(car)", &["error: car: wrong number of arguments: 0"]);
        check("(+ 1 \"a\")", &["error: +: not a fixnum: \"a\""]);
        check("()", &["error: empty combination: ()"]);
        check("(if 1)", &["error: if: bad syntax: (if 1)"]);
        check("(+ (define z 1) 2)", &["error: define: only allowed at top level: (define z 1)"]);
    }
}
