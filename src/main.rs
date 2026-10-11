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

/// The global allocator: mimalloc, as in s1, so benchmarks against s1
/// compare like with like.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use globals::Env;
use heap::Heap;
use io::PortKind;
use reader::{ParseError, parse};
use value::Value;

/// One top-level environment and its heap.
struct Session {
    heap: Heap,
    env: Env,
    vm: vm::Vm,
    dis: bool,
}

impl Session {
    fn new(dis: bool) -> Session {
        let mut heap = Heap::new();
        let mut env = Env::default();
        builtins::install(&mut heap, &mut env);
        Session { heap, env, vm: vm::Vm::default(), dis }
    }

    /// Compile one top-level form and run it.
    fn eval(&mut self, datum: Value) -> Result<Value, String> {
        let template = compile::compile(&mut self.heap, &mut self.env, datum)?;
        if self.dis {
            print!("{}", compile::disasm::disassemble(template));
        }
        self.vm.run(&mut self.heap, &mut self.env, template)
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
    fn procedures() {
        check("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 20)", &["fib", "6765"]);
        check("((lambda (x y) (cons y x)) 1 2)", &["(2 . 1)"]);
        check("(define (f) 42) (f) f (lambda (x) x)", &["f", "42", "#<procedure f>", "#<procedure>"]);
        check("(define g (lambda (x) x)) g", &["g", "#<procedure g>"]);
    }

    #[test]
    fn rest_arguments() {
        check("((lambda args args)) ((lambda args args) 1 2) ((lambda (a . r) (cons a r)) 1 2 3) ((lambda (a . r) r) 1)",
              &["()", "(1 2)", "(1 2 3)", "()"]);
        check("(define (f a b . c) c) (f 1 2) (f 1 2 3 4)", &["f", "()", "(3 4)"]);
    }

    #[test]
    fn closures_capture() {
        // Currying, composition, and a captured variable from two levels up.
        check("(define (add n) (lambda (x) (+ x n))) ((add 3) 4) (define add5 (add 5)) (add5 10) ((add 1) 1)",
              &["add", "7", "add5", "15", "2"]);
        check("(define (compose f g) (lambda (x) (f (g x)))) ((compose car cdr) '(1 2 3))", &["compose", "2"]);
        check("(define (curry3 f) (lambda (a) (lambda (b) (lambda (c) (f a b c))))) ((((curry3 +) 1) 2) 3)",
              &["curry3", "6"]);
        // Each closure has its own copy of the captured values.
        check("(define (k x) (lambda () x)) (define k1 (k 1)) (define k2 (k 2)) (k1) (k2)",
              &["k", "k1", "k2", "1", "2"]);
    }

    #[test]
    fn let_and_scope() {
        check("(let ((x 1) (y 2)) (+ x y)) (+ 1 (let ((x 2)) (* x 10)) 3)", &["3", "24"]);
        // The inits are evaluated outside the let's scope.
        check("(define x 10) (let ((x 1) (y x)) y)", &["x", "10"]);
        // Shadowing: inner bindings hide outer ones, globals and keywords.
        check("(let ((x 1)) (let ((x 2)) x)) ((lambda (car) car) 5) ((lambda (if) (if 1 2)) cons)",
              &["2", "5", "(1 . 2)"]);
        // Sibling lets reuse slots; the outer variable is untouched.
        check("(let ((a 1)) (+ (let ((b 2)) b) (let ((c 3)) c) a))", &["6"]);
        check("(let () 5)", &["5"]);
    }

    #[test]
    fn tail_calls_run_in_constant_space() {
        let mut s = Session::new(false);
        let mut port = io::new_string_port_input(
            "(define (loop n) (if (= n 0) (%frame-depth) (loop (- n 1)))) (loop 1000000)",
        );
        let mut last = Value::UNSPECIFIED;
        while let Ok(datum) = parse(&mut s.heap, &mut port) {
            last = s.eval(datum).unwrap();
        }
        assert_eq!(last.as_fixnum(), Some(0));
        let (stack, frames) = s.vm.high_water();
        assert!(stack < 64 && frames < 64, "stack {} frames {}", stack, frames);
        // A non-tail call does grow the frame stack.
        check("(define (deep n) (if (= n 0) (%frame-depth) (+ 0 (deep (- n 1))))) (deep 100)", &["deep", "100"]);
    }

    /// s1's `scheme/tail_tests.scm`, for the tail contexts P2 has: a loop
    /// reaches its base case at the same frame depth after 5 iterations as
    /// after 500 only if its self-call is a proper tail call.
    #[test]
    fn tail_contexts() {
        for (name, step) in [
            ("if", "(if #t (loop (- n 1)) 0)"),
            ("if, else branch", "(if #f 0 (loop (- n 1)))"),
            ("let", "(let ((m (- n 1))) (loop m))"),
            ("let body, last of several", "(let ((m (- n 1))) 1 (loop m))"),
            ("begin", "(begin 1 (loop (- n 1)))"),
            ("lambda body", "((lambda () (loop (- n 1))))"),
            ("lambda body, last of several", "((lambda () 1 (loop (- n 1))))"),
        ] {
            let src = format!("(define (loop n) (if (= n 0) (%frame-depth) {})) (= (loop 5) (loop 500))", step);
            assert_eq!(run(&src), ["loop", "#t"], "{}", name);
        }
        // The check itself: a call that isn't in tail position does grow.
        check("(define (tail-not n) (if (= n 0) (%frame-depth) (+ 0 (tail-not (- n 1))))) (< (tail-not 5) (tail-not 500))",
              &["tail-not", "#t"]);
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
        check("(define (f x) x) (f) (f 1 2) ((lambda (a . b) a))",
              &["f", "error: f: wrong number of arguments: 0 (expected 1)",
                "error: f: wrong number of arguments: 2 (expected 1)",
                "error: #<procedure>: wrong number of arguments: 0 (expected at least 1)"]);
        check("(lambda (x x) x)", &["error: lambda: duplicate parameter: x"]);
        check("(lambda (1) x)", &["error: lambda: bad parameter: 1"]);
        check("(lambda (x))", &["error: lambda: bad syntax: (lambda (x))"]);
        check("(let ((x 1) (x 2)) x)", &["error: let: duplicate variable: x"]);
        // An error deep in a call chain leaves the VM ready for the next form.
        check("(define (g n) (if (= n 0) (car 0) (+ 1 (g (- n 1))))) (g 10) (g 0) 7",
              &["g", "error: car: not a pair: 0", "error: car: not a pair: 0", "7"]);
    }
}
