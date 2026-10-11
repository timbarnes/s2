//! s2: a bytecode-compiled Scheme. See plan/ for the design.
//!
//! Usage:
//!
//! ```text
//! s2 [FLAGS]                  REPL on standard input
//! s2 [FLAGS] -e TEXT          evaluate each form in TEXT, writing each result
//! s2 [FLAGS] -f FILE          evaluate each form in FILE; an error is
//!                             reported and the next form runs
//! s2 [FLAGS] PROGRAM ARG ...  run a program: the first error ends it, with
//!                             status 70; (command-line) is PROGRAM ARG ...
//! ```
//!
//! Flags: `--expand` prints each form's core AST (after macro expansion),
//! and `--dis` its compiled code, before running it.

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
mod libraries;
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

use globals::{Env, env_of};
use heap::GcRef;
use heap::Heap;
use io::PortKind;
use reader::{ParseError, parse};
use value::Value;

/// The derived syntax (`cond`, `case`, `and`, `do`, …) as `syntax-rules`
/// macros, compiled into the binary and loaded into every session.
const CORE_SCM: &str = include_str!("../scheme/core.scm");

/// The heap, the VM, and the environment forms are compiled for.
struct Session {
    heap: Heap,
    /// The interaction environment, except while `scheme/core.scm` loads
    /// into the system environment.
    env: GcRef,
    vm: vm::Vm,
    dis: bool,
    expand: bool,
}

impl Session {
    fn new() -> Session {
        let mut heap = Heap::new();
        let core = Env::with_core_forms(&mut heap);
        let system = heap.environment(core);
        builtins::install(&mut heap, &mut env_of(system).borrow_mut());
        heap.system = Some(system);
        heap.interaction = Some(system);
        let vm = vm::Vm::new(&mut heap);
        let mut s = Session { heap, env: system, vm, dis: false, expand: false };
        let mut port = io::new_string_port_input(CORE_SCM);
        while let Ok(datum) = parse(&mut s.heap, &mut port) {
            if let Err(e) = s.eval(datum) {
                panic!("scheme/core.scm: {}", e);
            }
        }
        libraries::register_standard_libraries(&mut s.heap, system);
        // The interaction environment: every system binding, imported
        // (globals.rs).
        let mut interaction = Env::empty(true);
        for (name, binding) in env_of(system).borrow().bindings() {
            interaction.import(name, binding);
        }
        let interaction = s.heap.environment(interaction);
        s.heap.interaction = Some(interaction);
        s.env = interaction;
        s
    }

    /// Compile one top-level form and run it. If it imports a library
    /// whose file hasn't been loaded, load the file and try again.
    fn eval(&mut self, datum: Value) -> Result<Value, String> {
        loop {
            match compile::expand::expand(&mut self.heap, self.env, datum) {
                Ok(ast) => return self.run(&ast),
                Err(e) => match self.heap.libraries.pending_load.take() {
                    Some(path) => {
                        // The form waits through the load's collections.
                        self.heap.pinned.push(datum);
                        let loaded = self.load_library(&path);
                        self.heap.pinned.pop();
                        loaded?;
                    }
                    None => return Err(e),
                },
            }
        }
    }

    fn run(&mut self, ast: &compile::ast::Expr) -> Result<Value, String> {
        if self.expand {
            println!("{}", compile::ast::show(ast));
        }
        let template = compile::codegen::codegen(&mut self.heap, ast);
        if self.dis {
            print!("{}", compile::disasm::disassemble(template));
        }
        self.vm.run(&mut self.heap, template)
    }

    /// `(%load-library "path")`, written in Scheme (scheme/core.scm).
    fn load_library(&mut self, path: &std::path::Path) -> Result<Value, String> {
        let f = self.heap.intern("%load-library");
        let name = self.heap.string(&path.to_string_lossy());
        let form = self.heap.list(vec![f, name]);
        let saved = std::mem::replace(&mut self.env, self.heap.interaction.unwrap());
        let result = self.eval(form);
        self.env = saved;
        result
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
                Ok(v) if write_results => {
                    // Several values print one per line; none, nothing.
                    match v.as_obj().map(|o| &o.kind) {
                        Some(heap::Obj::Values(items)) => items.iter().for_each(|&v| println!("{}", printer::print_value(v))),
                        _ if v.is_unspecified() => {}
                        _ => println!("{}", printer::print_value(v)),
                    }
                }
                Ok(_) => {}
                Err(msg) => {
                    self.report(&msg);
                    ok = false;
                }
            }
        }
    }

    /// Print an error, and the backtrace of the procedures that were
    /// waiting, innermost first.
    fn report(&self, msg: &str) {
        std::io::stdout().flush().ok();
        eprintln!("error: {}", msg);
        for name in self.vm.backtrace.iter().take(10) {
            eprintln!("  in {}", name);
        }
        if self.vm.backtrace.len() > 10 {
            eprintln!("  ...");
        }
    }

    /// Run a program (R7RS §5.1): `args` is the file and its arguments,
    /// which `command-line` returns. The forms run in order in the
    /// interaction environment; the first error ends the program with
    /// status 70, as in s1. A first line starting `#!` is skipped, so a
    /// program can be a script.
    fn run_program(&mut self, args: &[String]) -> ! {
        builtins::set_command_line(args.to_vec());
        let text = match std::fs::read_to_string(&args[0]) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("s2: {}: {}", args[0], e);
                std::process::exit(EXIT_ERROR);
            }
        };
        let text = match text.strip_prefix("#!") {
            Some(rest) if !rest.starts_with("fold-case") && !rest.starts_with("no-fold-case") => {
                rest.find('\n').map_or("", |i| &rest[i..]).to_string()
            }
            _ => text,
        };
        let mut port = io::new_string_port_input(&text);
        loop {
            let result = match parse(&mut self.heap, &mut port) {
                Ok(datum) => self.eval(datum),
                Err(ParseError::Eof) => break,
                Err(ParseError::Syntax(msg)) => Err(msg),
            };
            if let Err(msg) = result {
                self.report(&msg);
                std::process::exit(EXIT_ERROR);
            }
        }
        std::io::stdout().flush().ok();
        #[cfg(feature = "stats")]
        vm::stats::report();
        std::process::exit(0)
    }
}

/// The exit status of a program that ends with an error (as in s1, after
/// BSD's EX_SOFTWARE).
const EXIT_ERROR: i32 = 70;

fn usage() -> ! {
    eprintln!("usage: s2 [--expand] [--dis] [-e TEXT | -f FILE | PROGRAM [ARG ...]]");
    std::process::exit(2);
}

/// The stack size of the thread s2 runs on. The expander, analysis and
/// code generation recurse over the code, and deeply nested code (a long
/// `or`, a `cond` with many clauses, which macros turn into nesting) needs
/// a deep stack: in a debug build, over 10KB per level. This is address
/// space, used only as deep as the code goes.
const STACK_SIZE: usize = 1 << 30;

fn main() {
    let session = std::thread::Builder::new().stack_size(STACK_SIZE).spawn(run_main).unwrap();
    session.join().unwrap();
}

fn run_main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut session = Session::new();
    while let Some(flag) = args.first() {
        match flag.as_str() {
            "--dis" => session.dis = true,
            "--expand" => session.expand = true,
            _ => break,
        }
        args.remove(0);
    }
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
        [flag, file] if flag == "-f" => match std::fs::read_to_string(file) {
            Ok(text) => session.eval_port(&mut io::new_string_port_input(&text), false, false),
            Err(e) => {
                eprintln!("s2: {}: {}", file, e);
                false
            }
        },
        [file, ..] if !file.starts_with('-') => session.run_program(&args),
        _ => usage(),
    };
    std::io::stdout().flush().ok();
    #[cfg(feature = "stats")]
    vm::stats::report();
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Evaluate each form in `src` and write each result, or `error: msg`.
    /// The collector runs at every safepoint and poisons what it frees, so
    /// every result test is also a GC test.
    fn run(src: &str) -> Vec<String> {
        let mut s = Session::new();
        s.heap.stress = true;
        s.heap.poison = true;
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
        check("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 15)", &["fib", "610"]);
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
        // Captured heap objects and a lambda's quoted constants survive
        // collections: reachable only through the closure and its template.
        check("(define (k x) (lambda () x)) (define kp (k (cons 1 (cons \"s\" '())))) (kp) (define (q) '(a #(b \"c\"))) (q)",
              &["k", "kp", "(1 \"s\")", "q", "(a #(b \"c\"))"]);
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
    fn assignment() {
        // Globals; set! returns nothing printable.
        check("(define g 1) (set! g (+ g 1)) g (set! nope 1)", &["g", "", "2", "error: set!: unbound variable: nope"]);
        // A parameter assigned but not captured stays in its slot.
        check("(define (f x) (set! x (+ x 1)) (* x 10)) (f 1)", &["f", "20"]);
        // A counter: each closure has its own boxed variable.
        check("(define (make-counter) (let ((n 0)) (lambda () (set! n (+ n 1)) n)))
               (define c (make-counter)) (define d (make-counter)) (c) (c) (d) (c)",
              &["make-counter", "c", "d", "1", "2", "1", "3"]);
        // Two closures share one variable; so does the procedure that made them.
        check("(define (cell v) (let ((get (lambda () v)) (put (lambda (x) (set! v x)))) (put (+ v 1)) (cons get put)))
               (define p (cell 1)) ((car p)) ((cdr p) 42) ((car p))",
              &["cell", "p", "2", "", "42"]);
        // A captured parameter that is assigned is boxed at entry.
        check("(define (acc total) (lambda (x) (set! total (+ total x)) total)) (define a (acc 100)) (a 10) (a 10)",
              &["acc", "a", "110", "120"]);
        check("(set! 1 2)", &["error: set!: bad syntax: (set! 1 2)"]);
    }

    #[test]
    fn letrec_and_internal_definitions() {
        check("(define (parity n)
                 (define (even? n) (if (= n 0) #t (odd? (- n 1))))
                 (define (odd? n) (if (= n 0) #f (even? (- n 1))))
                 (cons (even? n) (odd? n)))
               (parity 7) (parity 1000)",
              &["parity", "(#f . #t)", "(#t . #f)"]);
        check("(letrec ((fact (lambda (n) (if (= n 0) 1 (* n (fact (- n 1))))))) (fact 20))", &["2432902008176640000"]);
        // letrec*: each init sees the earlier variables.
        check("(letrec* ((a 1) (b (+ a 1))) (cons a b))", &["(1 . 2)"]);
        check("(define (f) (define a 1) (define b (+ a 1)) (define (g) (* a b)) (g)) (f)", &["f", "2"]);
        // A reference before the init has run is an error, even through a lambda.
        check("(letrec ((a b) (b 1)) a)", &["error: variable used before its initialisation: b"]);
        check("(letrec ((a (lambda () b)) (b (a))) b)", &["error: variable used before its initialisation: b"]);
        // A procedure defined internally is named after its definition.
        check("(define (f) (define (inner) 1) inner) (f)", &["f", "#<procedure inner>"]);
        // Shadowing a global with an internal definition.
        check("(define x 'global) (define (f) (define x 'local) x) (f) x", &["x", "f", "local", "global"]);
        check("(define (f) (define a 1) (display a) (define b 2) b)",
              &["error: define: only allowed at top level or at the start of a body: (define b 2)"]);
        check("(define (f) (define a 1))", &["error: body: no expression after the definitions"]);
        check("(define (f) (define a 1) (define a 2) a)", &["error: define: duplicate definition: a"]);
        check("(letrec ((a 1) (a 2)) a) (letrec ((a)) a)",
              &["error: letrec: duplicate variable: a", "error: letrec: bad binding: (a)"]);
    }

    #[test]
    fn named_let() {
        check("(let loop ((i 0) (acc '())) (if (= i 5) acc (loop (+ i 1) (cons i acc))))", &["(4 3 2 1 0)"]);
        // The inits are outside the loop's scope; the name is visible in the body.
        check("(define i 10) (let loop ((i i)) (if (= i 0) loop (loop (- i 1))))", &["i", "#<procedure loop>"]);
        check("(let loop ((i 0)) (if (= i 2000) (%frame-depth) (loop (+ i 1))))", &["0"]);
        check("(let loop) (let loop ((x 1) (x 2)) x)",
              &["error: let: bad syntax: (let loop)", "error: let: duplicate variable: x"]);
    }

    /// s1's `syntax_rules_tests.scm`, the cases that need only P5.
    #[test]
    fn hygiene() {
        let swap = "(define-syntax swap! (syntax-rules () ((_ a b) (let ((tmp a)) (set! a b) (set! b tmp)))))";
        check(&format!("{} (let ((x 1) (y 2)) (swap! x y) (list x y))", swap), &["swap!", "(2 1)"]);
        // The template's tmp doesn't capture the user's.
        check(&format!("{} (let ((tmp 1) (other 2)) (swap! tmp other) (list tmp other))", swap), &["swap!", "(2 1)"]);
        // A template's keywords mean what they meant where the macro was defined.
        check("(define-syntax my-if (syntax-rules () ((_ c t e) (cond (c t) (else e)))))
               (let ((else #f) (cond 'shadowed)) (my-if #t 'yes 'no))", &["my-if", "yes"]);
        check("(let ((x 'outer)) (let-syntax ((m (syntax-rules () ((m) x)))) (let ((x 'inner)) (m))))", &["outer"]);
        check("(let-syntax ((when (syntax-rules () ((_ test s1 s2 ...) (if test (begin s1 s2 ...))))))
                 (let ((if #t)) (when if (set! if 'now)) if))", &["now"]);
        // A recursive macro whose temporaries and keywords the user shadows.
        check("(define-syntax my-or (syntax-rules () ((_) #f) ((_ e) e) ((_ e1 e2 ...) (let ((temp e1)) (if temp temp (my-or e2 ...))))))
               (let ((x #f) (y 7) (temp 8) (let null?) (if pair?)) (my-or x (let temp) (if y) y))", &["my-or", "7"]);
        // Literals match by binding: a local => is a variable.
        check("(define-syntax arrow (syntax-rules (=>) ((_ a => b) 'arrow) ((_ a b c) 'plain)))
               (arrow 1 => 2) (let ((=> 0)) (arrow 1 => 2)) (let ((=> #f)) (cond (#t => 'ok)))",
              &["arrow", "arrow", "plain", "ok"]);
        check("(let ((else #f)) (cond (else 1) (#t 2)))", &["2"]);
        // set! through an alias assigns the definition scope's variable.
        check("(define counter 0) (define-syntax bump! (syntax-rules () ((_) (set! counter (+ counter 1)))))
               (let ((counter 'shadowed)) (bump!) (bump!) counter) counter",
              &["counter", "bump!", "shadowed", "2"]);
        // Quoted template data is plain symbols.
        check("(define-syntax quoted (syntax-rules () ((_ x) '(a x #(b c))))) (quoted 1) (eq? (car (quoted 1)) 'a)",
              &["quoted", "(a 1 #(b c))", "#t"]);
        check("(define-syntax qq (syntax-rules () ((_ e) `(tag ,e ,(list 'x e))))) (qq 5)", &["qq", "(tag 5 (x 5))"]);
    }

    #[test]
    fn macro_defining_macros_and_scope() {
        check("(define-syntax be-like-begin (syntax-rules () ((_ name) (define-syntax name (syntax-rules () ((name expr (... ...)) (begin expr (... ...))))))))
               (be-like-begin sequence) (sequence 0 1 2 3)", &["be-like-begin", "sequence", "3"]);
        check("(define-syntax bld (syntax-rules () ((_ name) (define-syntax name (syntax-rules dots () ((name expr dots) (begin expr dots)))))))
               (bld seq) (seq 2 3 4 5)", &["bld", "seq", "5"]);
        // An introduced top-level definition is visible to the macro the
        // same expansion defines, and hidden from everything else.
        check("(define-syntax jabberwocky (syntax-rules () ((_ hatter) (begin (define march-hare 42) (define-syntax hatter (syntax-rules () ((_) march-hare)))))))
               (jabberwocky mad-hatter) (mad-hatter) march-hare",
              &["jabberwocky", "mad-hatter", "42", "error: unbound variable: march-hare"]);
        check("(let ((x 1) (y 2)) (define-syntax local-swap! (syntax-rules () ((_ a b) (let ((t a)) (set! a b) (set! b t))))) (local-swap! x y) (list x y))",
              &["(2 1)"]);
        check("(let () (define x 1) (let-syntax () (define x 2) #f) x)", &["1"]);
        // A macro in a body can refer to a procedure defined after it.
        check("(let () (define-syntax fwd (syntax-rules () ((_) (later)))) (define (use) (fwd)) (define (later) 42) (use))", &["42"]);
        // A macro use that expands into definitions, and begin splicing.
        check("(define-syntax def2 (syntax-rules () ((_ a b v) (begin (define a v) (define b v)))))
               (let () (def2 p q 5) (begin (define r 1)) (+ p q r))", &["def2", "11"]);
        // Transformers that refer to each other. (Expansion doesn't run the
        // `if`s, so they can't recurse into each other forever.)
        check("(letrec-syntax ((ev? (syntax-rules () ((_ n) (if (= n 0) #t (od? (- n 1))))))
                               (od? (syntax-rules () ((_ n) (not (= n 0))))))
                 (list (ev? 4) (od? 0)))", &["(#t #f)"]);
        // Redefining a macro, and a core form, as variables.
        check("(define-syntax m (syntax-rules () ((_) 1))) (define m 2) m (define (if x) x) (if 5)", &["m", "m", "2", "if", "5"]);
    }

    #[test]
    fn derived_forms() {
        check("(and) (and 1 2) (and 1 #f 3) (or) (or #f 2) (or #f #f)", &["#t", "2", "#f", "#f", "2", "#f"]);
        check("(when #t 1 2) (when #f 1) (unless #f 3) (unless #t 3)", &["2", "", "3", ""]);
        check("(let* ((a 1) (b (+ a 1))) (list a b)) (let* () 5)", &["(1 2)", "5"]);
        check("(cond (#f 1) ((+ 1 1) => (lambda (x) (* x 10))) (else 3)) (cond ((car '(7)))) (cond (#f 1))",
              &["20", "7", ""]);
        check("(case (* 2 3) ((2 3 5 7) 'prime) ((1 4 6 8 9) 'composite)) (case 3 ((3) => (lambda (x) (+ x 1))) (else 'b)) (case 'z ((a) 1) (else => (lambda (x) x)))",
              &["composite", "4", "z"]);
        check("(do ((i 0 (+ i 1)) (acc '() (cons i acc))) ((= i 3) acc))", &["(2 1 0)"]);
        // do's loop variable can't capture the user's.
        check("(let ((loop 'mine)) (do ((i 0 (+ i 1)) (acc '() (cons loop acc))) ((= i 2) acc)))", &["(mine mine)"]);
        check("(let ((if #f)) (do ((i 0 (+ i 1))) ((= i 2) 'done)))", &["done"]);
    }

    #[test]
    fn quasiquote() {
        check("`(1 ,(+ 1 1) ,@(list 3 4)) `#(1 ,(+ 1 1)) `(a . ,(+ 1 2)) `(x ,@'() y) ``(a ,(b ,(+ 1 2))) `,5 `abc",
              &["(1 2 3 4)", "#(1 2)", "(a . 3)", "(x y)", "(quasiquote (a (unquote (b 3))))", "5", "abc"]);
        // Constant parts stay constant, and a local quote doesn't matter.
        check("(let ((quote list)) `(x y))", &["(x y)"]);
        check("(unquote 1) `,@(list 1)", &["error: unquote: not in a quasiquote: (unquote 1)", "error: unquote-splicing: not in a list: (unquote-splicing (list 1))"]);
    }

    #[test]
    fn apply_spreads_its_last_argument() {
        check("(apply + 1 2 '(3 4)) (apply list '()) (apply apply (list + (list 1 2))) (apply car '((a)))",
              &["10", "()", "3", "a"]);
        check("(apply +) (apply + 1 2) (apply 5 '())",
              &["error: apply: wrong number of arguments: 1", "error: apply: not a proper list: 2", "error: not a procedure: 5"]);
        // map and friends over several lists go through apply.
        check("(map + '(1 2 3) '(10 20)) (vector-map * #(1 2) #(3 4))", &["(11 22)", "#(3 8)"]);
    }

    /// Records, string ports and the numeric tower, under GC stress (as
    /// every `check` is). s1's data and port tests cover the procedures in
    /// detail (tests/scheme).
    #[test]
    fn data_types() {
        check("(define-record-type point (make-point x y) point? (x px) (y py set-py!))
               (define p (make-point 3 4)) (set-py! p 5) (list (px p) (py p) (point? p) (point? 5)) p (px 7)",
              &["set-py!", "p", "", "(3 5 #t #f)", "#<point 3 5>", "error: record accessor: expected a point record, got 7"]);
        check("(let ((out (open-output-string))) (write 'a out) (display \" b\" out) (get-output-string out))
               (let ((in (open-input-string \"(1 2) x\"))) (list (read in) (read in) (eof-object? (read in))))",
              &["\"a b\"", "((1 2) x #t)"]);
        check("(/ 1 3) (+ 1/2 1/2) (* 1.5 2) (exact 2.5) (expt 2 100) (exact->inexact 1/3) (sqrt 16) (sqrt -1)",
              &["1/3", "1", "3.0", "5/2", "1267650600228229401496703205376", "0.3333333333333333", "4",
                "error: sqrt: complex results are not supported"]);
        check("(quotient 17 -5) (remainder 17 -5) (modulo 17 -5) (quotient 1152921504606846976 2) (exact-integer? (- (expt 2 62) (expt 2 62)))",
              &["-3", "2", "-3", "576460752303423488", "#t"]);
    }

    #[test]
    fn continuations() {
        // Escape, then re-entry three times: an assigned variable keeps its
        // latest value across re-entries (it is boxed).
        check("(+ 1 (call/cc (lambda (k) (+ 10 (k 1)))))
               (let ((k #f) (log '())) (let ((v (call/cc (lambda (c) (set! k c) 0)))) (set! log (cons v log)) (if (< v 3) (k (+ v 1))) log))",
              &["2", "(3 2 1 0)"]);
        // Values through continuations; a continuation printed.
        check("(call-with-values (lambda () (call/cc (lambda (k) (k 1 2)))) list) (call/cc (lambda (k) k)) (values 1 2)",
              &["(1 2)", "#<procedure>", "1 2"]);
        // A continuation captured in one top-level form, re-entered from the
        // next: the old form finishes, and its value is the new form's.
        check("(define r '()) (define k #f) (set! r (cons (call/cc (lambda (c) (set! k c) 1)) r)) r (k 2) r",
              &["r", "k", "", "(1)", "", "(2 1)"]);
        check("(define e (%call/ec (lambda (k) k))) (e 1)", &["e", "error: escape continuation invoked after its extent has ended"]);
    }

    #[test]
    fn exceptions() {
        // Errors from builtins and from the VM are raised, so guard catches them.
        check("(guard (e ((error-object? e) (list (error-object-message e) (error-object-irritants e)))) (car 1))
               (guard (e (#t 'unbound)) nope) (guard (e (#t 'arity)) ((lambda (x) x))) (guard (e (#t 'not-proc)) (1 2))
               (guard (e ((string? e) e)) (raise \"s\")) (guard (e ((symbol? e) e)) (error \"x\"))",
              &["(\"car: expected a pair, got 1\" ())", "unbound", "arity", "not-proc", "\"s\"", "error: x"]);
        check("(guard (e (#t (list (file-error? e) (error-object-irritants e)))) (open-input-file \"/no/such/file\"))
               (guard (e (#t (read-error? e))) (read (open-input-string \")\")))",
              &["(#t (\"/no/such/file\"))", "#t"]);
        // Uncaught: the message (with irritants written), and the after
        // thunks have run.
        check("(define log '()) (dynamic-wind (lambda () #f) (lambda () (error \"bad thing:\" 1 \"x\")) (lambda () (set! log 'after-ran))) log
               (raise 'boom) (with-exception-handler (lambda (c) 0) (lambda () (raise 1)))",
              &["log", "error: bad thing: 1 \"x\"", "after-ran", "error: uncaught exception: boom",
                "error: exception handler returned from a non-continuable raise of 1"]);
        check("(with-exception-handler (lambda (c) 42) (lambda () (+ (raise-continuable 'oops) 23)))", &["65"]);
    }

    #[test]
    fn uncaught_errors_have_a_backtrace() {
        let mut s = Session::new();
        let mut port = io::new_string_port_input("(define (f x) (car x)) (define (g x) (+ 1 (f x))) (g 5)");
        let mut last = Ok(Value::UNSPECIFIED);
        while let Ok(datum) = parse(&mut s.heap, &mut port) {
            last = s.eval(datum);
        }
        assert_eq!(last.err().as_deref(), Some("car: expected a pair, got 5"));
        assert_eq!(s.vm.backtrace, ["f", "g"]);
    }

    /// Guards nested 8,000 deep, returning normally and re-raising through
    /// every level, make no full continuation captures (each guard costs
    /// constant time; with the reference implementation each would copy
    /// the stack, which is quadratic). s1 once had a cubic bug here.
    #[test]
    fn nested_guards_are_linear() {
        let (s, last) = session_after(
            "(define (nested-guards n) (if (= n 0) 0 (+ 1 (guard (e (#t 'caught)) (nested-guards (- n 1))))))
             (define (nested-reraise n) (if (= n 0) (raise 'deep) (guard (e ((eq? e 'never) 0)) (nested-reraise (- n 1)))))
             (list (nested-guards 8000) (guard (e (#t (list 'outer e))) (nested-reraise 8000)))",
        );
        assert_eq!(printer::print_value(last), "(8000 (outer deep))");
        assert_eq!(s.vm.captures, 0);
    }

    /// Libraries and environments, under GC stress (as every `check` is).
    /// tests/scheme/library_tests.scm, ported from s1, covers them in
    /// detail.
    #[test]
    fn libraries_and_eval() {
        // A library file found on the search path, loading the one it
        // imports, while the form that imported it waits through the
        // collections (it is pinned).
        check("(import (tests libs uses-greet)) (greet-twice \"a\") (import (tests libs greet)) (greet \"b\")",
              &["", "(\"hello, a\" \"hello, a\")", "", "\"hello, b\""]);
        check("(eval '(* 7 3) (interaction-environment)) (eval '(car '(1)) (environment '(only (scheme base) car quote)))
               (eval '(define z 1) (environment '(scheme base))) (eval 'cons (environment '(only (scheme base) car)))",
              &["21", "1", "error: define: this environment is immutable", "error: unbound variable: cons"]);
        // An imported variable sees the library's assignments; the importer
        // can't assign it, and its own define shadows it.
        check("(define-library (t c) (export n inc!) (import (scheme base)) (begin (define n 0) (define (inc!) (set! n (+ n 1)))))
               (import (t c)) (inc!) n (set! n 5) (define n 'mine) n",
              &["", "", "", "1", "error: set!: n is imported and can't be assigned", "n", "mine"]);
        check("(define (f) (import (scheme base))) (cond-expand ((and r7rs (not no-such)) 'yes))",
              &["error: import: only allowed at top level", "yes"]);
    }

    #[test]
    fn deep_expansions() {
        // A 1,000-argument or nests 1,000 macro uses, and as many lets: on
        // a default 2MB thread stack, a debug build overflowed at about 150.
        let src = format!("(or {} 1)", "#f ".repeat(1000));
        check(&src, &["1"]);
        // A macro that expands forever is an error, not a crash.
        check("(define-syntax forever (syntax-rules () ((_ x) (list (forever x))))) (forever 1)",
              &["forever", "error: macro uses nested more than 10000 deep: (forever 1)"]);
    }

    #[test]
    fn macro_errors() {
        check("(define-syntax swap! (syntax-rules () ((_ a b) (list a b)))) (swap! 1)", &["swap!", "error: no syntax rule matches (swap! 1)"]);
        check("(define-syntax broken (syntax-rules () ((_ ... x) 1)))",
              &["error: syntax-rules: ellipsis must follow a pattern in (... x)"]);
        check("(define-syntax m 5) (syntax-rules ()) if (set! if 1)",
              &["error: not a syntax-rules transformer: 5", "error: syntax-rules: only allowed as a macro's transformer: (syntax-rules ())",
                "error: if: syntax used as a variable", "error: set!: can't assign to syntax: if"]);
        check("(let () (define-syntax m (syntax-rules () ((_) 1))) (define m 2) m)", &["error: define: duplicate definition: m"]);
    }

    /// `--expand` output: the core AST, with renamed variables numbered.
    #[test]
    fn expand_shows_the_core_ast() {
        let mut s = Session::new();
        let mut show = |src: &str| {
            let datum = parse(&mut s.heap, &mut io::new_string_port_input(src)).unwrap();
            compile::ast::show(&compile::expand::expand(&mut s.heap, s.env, datum).unwrap())
        };
        assert_eq!(show("(define (f x) (or x 'none))"),
                   "(define f (named-lambda (f x.1) (let ((x.2 x.1)) (if x.2 x.2 'none))))");
        assert_eq!(show("(let loop ((i 0)) (when (< i 3) (loop (+ i 1))))"),
                   "((letrec* ((loop.1 (named-lambda (loop i.2) (if (< i.2 3) (loop.1 (+ i.2 1)) #<unspecified>)))) loop.1) 0)");
        assert_eq!(show("`(a ,b)"), "(cons 'a (cons b '()))");
        assert_eq!(show("(lambda args (set! args 1) args)"), "(lambda args.1 (begin (set! args.1 1) args.1))");
    }

    #[test]
    fn tail_calls_run_in_constant_space() {
        let mut s = Session::new();
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

    /// Evaluate `src` in a fresh session with the default GC schedule and
    /// return the session and the last result.
    fn session_after(src: &str) -> (Session, Value) {
        let mut s = Session::new();
        s.heap.stress = false;
        let mut port = io::new_string_port_input(src);
        let mut last = Value::UNSPECIFIED;
        while let Ok(datum) = parse(&mut s.heap, &mut port) {
            last = s.eval(datum).unwrap();
        }
        (s, last)
    }

    #[test]
    fn allocating_loop_runs_in_flat_memory() {
        let (s, last) = session_after(
            "(define (loop n) (if (= n 0) 'done (begin (cons n (cons n '())) (loop (- n 1))))) (loop 1000000)",
        );
        assert_eq!(printer::print_value(last), "done");
        // Two million pairs were made; the heap never held more than one
        // collection interval's worth.
        assert!(s.heap.collections > 50, "{} collections", s.heap.collections);
        assert!(s.heap.len() < 25_000, "{} objects", s.heap.len());
    }

    #[test]
    fn deep_recursion_collects_in_linear_time() {
        // Each of the 100,000 frames keeps a pair alive in a `let` slot, so
        // the live heap grows throughout. Marking work stays within a small
        // multiple of the allocation; a fixed collection interval would
        // re-mark the growing heap every interval (3n here, quadratic in
        // general).
        let n = 100_000;
        let (s, last) = session_after(&format!(
            "(define (deep n) (let ((p (cons n '()))) (if (= n 0) 0 (+ (car p) (deep (- n 1)) (- 0 (car p)) 1)))) (deep {})",
            n
        ));
        assert_eq!(last.as_fixnum(), Some(n));
        assert!(s.heap.collections >= 2, "{} collections", s.heap.collections);
        assert!(s.heap.marked < 2 * n as usize + 20_000, "{} marked", s.heap.marked);
    }

    /// s1's `scheme/tail_tests.scm`: a loop
    /// reaches its base case at the same frame depth after 5 iterations as
    /// after 100 only if its self-call is a proper tail call. All of s1's
    /// contexts are here now.
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
            ("letrec", "(letrec ((m (- n 1))) (loop m))"),
            ("letrec*", "(letrec* ((m (- n 1))) (loop m))"),
            ("named let", "(let l ((m (- n 1))) (loop m))"),
            ("internal definitions", "((lambda () (define m (- n 1)) (loop m)))"),
            ("set! then call", "((lambda (m) (set! m (- m 1)) (loop m)) n)"),
            ("cond", "(cond (#f 0) (else (loop (- n 1))))"),
            ("cond =>", "(cond ((- n 1) => loop))"),
            ("case", "(case 1 ((1) (loop (- n 1))))"),
            ("case else", "(case 1 ((2) 0) (else (loop (- n 1))))"),
            ("case =>", "(case 1 ((1) => (lambda (x) (loop (- n 1)))))"),
            ("and, two operands", "(and #t (loop (- n 1)))"),
            ("and, last of several", "(and #t 1 (loop (- n 1)))"),
            ("or, two operands", "(or #f (loop (- n 1)))"),
            ("or, last of several", "(or #f #f (loop (- n 1)))"),
            ("when", "(when #t (loop (- n 1)))"),
            ("unless", "(unless #f (loop (- n 1)))"),
            ("let*", "(let* ((m (- n 1))) (loop m))"),
            ("let-syntax", "(let-syntax () (loop (- n 1)))"),
            ("letrec-syntax", "(letrec-syntax () (loop (- n 1)))"),
            ("do result", "(do ((i 0 (+ i 1))) ((= i 1) (loop (- n 1))))"),
            ("apply", "(apply loop (list (- n 1)))"),
            ("call/cc", "(call/cc (lambda (k) (loop (- n 1))))"),
            ("call-with-values consumer", "(call-with-values (lambda () (- n 1)) loop)"),
            ("let-values", "(let-values (((m) (- n 1))) (loop m))"),
            ("let*-values", "(let*-values (((m) (- n 1))) (loop m))"),
            ("case-lambda", "((case-lambda ((x) (loop x))) (- n 1))"),
            ("apply, case-lambda", "(apply (case-lambda ((x) (loop x))) (list (- n 1)))"),
            ("eval", "(eval (list 'loop (- n 1)) (interaction-environment))"),
            ("macro use", "(let-syntax ((tail-id (syntax-rules () ((_ e) e)))) (tail-id (loop (- n 1))))"),
        ] {
            let src = format!("(define (loop n) (if (= n 0) (%frame-depth) {})) (= (loop 5) (loop 100))", step);
            assert_eq!(run(&src), ["loop", "#t"], "{}", name);
        }
        // The check itself: a call that isn't in tail position does grow.
        check("(define (tail-not n) (if (= n 0) (%frame-depth) (+ 0 (tail-not (- n 1))))) (< (tail-not 5) (tail-not 100))",
              &["tail-not", "#t"]);
    }

    #[test]
    fn errors_abort_the_form_only() {
        check("y (define y 5) y", &["error: unbound variable: y", "y", "5"]);
        check("(car 1)", &["error: car: expected a pair, got 1"]);
        check("(1 2)", &["error: not a procedure: 1"]);
        check("(car)", &["error: car: wrong number of arguments: 0"]);
        check("(+ 1 \"a\")", &["error: +: expected a number, got \"a\""]);
        check("()", &["error: empty combination: ()"]);
        check("(if 1)", &["error: if: bad syntax: (if 1)"]);
        check("(+ (define z 1) 2)", &["error: define: only allowed at top level or at the start of a body: (define z 1)"]);
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
              &["g", "error: car: expected a pair, got 0", "error: car: expected a pair, got 0", "7"]);
    }
}
