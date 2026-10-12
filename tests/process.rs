//! R7RS 6.14 procedures that end or depend on the process (exit,
//! emergency-exit, command-line), and running programs, tested by running
//! the s2 binary. Forked from s1's tests/process.rs; s1's test of loading
//! files before a script (`-f lib script`) is left out, since s2's command
//! line doesn't offer that.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

/// Write `source` to a temporary file named after `name`, and return its path.
fn script(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("s2-process-test-{}-{}.scm", std::process::id(), name));
    std::fs::write(&path, source).unwrap();
    path
}

/// Run s2 with `args`, from the package root, with standard input closed.
fn s2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_s2"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// Load `source` as a file with -f: the exit status and stdout.
fn run_file(name: &str, source: &str) -> (i32, String) {
    let path = script(name, source);
    let out = s2(&["-f", path.to_str().unwrap()]);
    std::fs::remove_file(&path).ok();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn exit_statuses() {
    assert_eq!(run_file("exit-none", "(exit)").0, 0);
    assert_eq!(run_file("exit-true", "(exit #t)").0, 0);
    assert_eq!(run_file("exit-false", "(exit #f)").0, 1);
    assert_eq!(run_file("exit-int", "(exit 7)").0, 7);
    assert_eq!(run_file("emergency-int", "(emergency-exit 4)").0, 4);
    assert_eq!(run_file("emergency-false", "(emergency-exit #f)").0, 1);
}

#[test]
fn exit_stops_the_program() {
    let (code, out) = run_file("exit-stops", "(display \"a\") (exit 2) (display \"b\")");
    assert_eq!((code, out.as_str()), (2, "a"));
}

#[test]
fn exit_runs_after_thunks_innermost_first() {
    let (code, out) = run_file(
        "exit-winds",
        r#"(dynamic-wind
             (lambda () (display "in "))
             (lambda ()
               (dynamic-wind (lambda () #f)
                             (lambda () (exit 3))
                             (lambda () (display "inner "))))
             (lambda () (display "outer")))"#,
    );
    assert_eq!((code, out.as_str()), (3, "in inner outer"));
}

#[test]
fn emergency_exit_skips_after_thunks() {
    let (code, out) = run_file(
        "emergency-winds",
        r#"(display "start ")
           (dynamic-wind (lambda () #f)
                         (lambda () (emergency-exit 5))
                         (lambda () (display "after")))"#,
    );
    assert_eq!((code, out.as_str()), (5, "start "));
}

#[test]
fn exit_rejects_other_values() {
    let (code, out) = run_file(
        "exit-bad",
        r#"(display (guard (e (#t (error-object-message e))) (exit 'x)))"#,
    );
    assert_eq!((code, out.as_str()), (0, "exit: expected a boolean or an exact integer, got x"));
}

/// Run `source` as a program, `s2 program args...`: the exit status,
/// stdout and stderr, and the program's path.
fn run_script(name: &str, source: &str, args: &[&str]) -> (i32, String, String, String) {
    let path = script(name, source);
    let path_str = path.to_str().unwrap().to_string();
    let mut all = vec![path_str.as_str()];
    all.extend_from_slice(args);
    let out = s2(&all);
    std::fs::remove_file(&path).ok();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        path_str,
    )
}

#[test]
fn command_line_without_a_script() {
    let (code, out) = run_file("cl-none", "(write (command-line))");
    assert_eq!((code, out.as_str()), (0, "(\"s2\")"));
}

#[test]
fn script_gets_its_arguments() {
    // Everything after the script goes to it, options included.
    let (code, out, _, path) =
        run_script("cl-args", "(write (command-line))", &["a", "-q", "--x", "two words"]);
    assert_eq!(code, 0);
    // write escapes the backslashes in a Windows path.
    let path = path.replace('\\', "\\\\");
    assert_eq!(out, format!("(\"{}\" \"a\" \"-q\" \"--x\" \"two words\")", path));
}

#[test]
fn script_skips_a_shebang_line() {
    let (code, out, _, _) = run_script("shebang", "#!/usr/bin/env s2\n(display \"ran\")\n", &[]);
    assert_eq!((code, out.as_str()), (0, "ran"));
}

#[test]
fn uncaught_error_ends_a_script_with_status_70() {
    let (code, out, err, _) = run_script(
        "uncaught",
        r#"(dynamic-wind (lambda () #f)
                         (lambda () (car 1))
                         (lambda () (display "after")))
           (display "never")"#,
        &[],
    );
    assert_eq!((code, out.as_str()), (70, "after"));
    assert!(err.contains("car"), "{:?}", err);
}

#[test]
fn syntax_error_ends_a_script_with_status_70() {
    let (code, out, _, _) = run_script("syntax", "(display \"x\")\n#bogus\n(display \"never\")", &[]);
    assert_eq!(code, 70);
    assert!(out.starts_with("x") && !out.contains("never"), "{:?}", out);
}

#[test]
fn script_exit_status() {
    assert_eq!(run_script("script-exit", "(exit 9)", &[]).0, 9);
    assert_eq!(run_script("script-ok", "(display 1)", &[]).0, 0);
}

/// Run s2 with `args` and `S2_LIBRARY_PATH` set to `libs`: the exit
/// status and stdout.
fn s2_with_libs(args: &[&str], libs: &std::path::Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_s2"))
        .args(args)
        .env("S2_LIBRARY_PATH", libs)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn compiled_programs() {
    // Compiling runs nothing; the compiled program behaves as the source
    // does, with its own arguments, and an error still ends it with 70.
    let path = script("compiled", "(display (cdr (command-line))) (newline) (car '()) (display \"not reached\")");
    let out = path.with_extension("s2o");
    let compile = s2(&["--compile", path.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!((compile.status.code(), compile.stdout.as_slice()), (Some(0), &b""[..]));
    let from_source = s2(&[path.to_str().unwrap(), "a", "b"]);
    let compiled = s2(&[out.to_str().unwrap(), "a", "b"]);
    assert_eq!(String::from_utf8_lossy(&compiled.stdout), "(a b)\n");
    assert_eq!((compiled.status.code(), &compiled.stdout, &compiled.stderr), (from_source.status.code(), &from_source.stdout, &from_source.stderr));
    assert_eq!(compiled.status.code(), Some(70));
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&out).ok();
}

#[test]
fn library_files_are_cached_until_they_change() {
    let dir = std::env::temp_dir().join(format!("s2-cache-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("cachetest")).unwrap();
    let lib = dir.join("cachetest").join("m.sld");
    let cache = lib.with_extension("s2o");
    let define = |v: u32| {
        let text = format!("(define-library (cachetest m) (export v) (import (scheme base)) (begin (define v {})))", v);
        std::fs::write(&lib, text).unwrap();
    };
    let program = script("cache", "(import (scheme write) (cachetest m)) (display v)");
    let run = || s2_with_libs(&[program.to_str().unwrap()], &dir);
    define(1);
    assert_eq!(run(), (0, "1".into()));
    let first = std::fs::metadata(&cache).unwrap().modified().unwrap();
    assert_eq!(run(), (0, "1".into()));
    assert_eq!(std::fs::metadata(&cache).unwrap().modified().unwrap(), first, "the cache is used, not rewritten");
    define(2);
    assert_eq!(run(), (0, "2".into()), "a changed library is compiled again");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&program).ok();
}

#[test]
fn unclosed_file_output_is_written_however_the_program_ends() {
    // File output is buffered; the buffer must reach the file even when the
    // port is never closed.
    for (name, ending, status) in [("end", "", 0), ("exit", "(exit 3)", 3), ("emergency", "(emergency-exit 4)", 4), ("error", "(car '())", 70)] {
        let out = std::env::temp_dir().join(format!("s2-unclosed-{}-{}.txt", std::process::id(), name));
        let program = script(
            &format!("unclosed-{}", name),
            &format!("(define p (open-output-file {:?})) (write-string \"kept\" p) (write-char #\\! p) {}", out.to_str().unwrap(), ending),
        );
        assert_eq!(s2(&[program.to_str().unwrap()]).status.code(), Some(status), "{}", name);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept!", "{}", name);
        std::fs::remove_file(&out).ok();
        std::fs::remove_file(&program).ok();
    }
}

/// Output into a pipe whose reader has gone ends s2 quietly, by SIGPIPE,
/// as it ends `cat`, rather than with a panic.
#[cfg(unix)]
#[test]
fn closed_output_pipe_ends_s2_quietly() {
    use std::os::unix::process::ExitStatusExt;
    let mut child = Command::new(env!("CARGO_BIN_EXE_s2"))
        .args(["-e", "(let loop () (display \"x\") (loop))"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.signal(), Some(13), "{:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
}
