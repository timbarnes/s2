//! The s2 binary: `-e`, standard input, files, exit status.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn s2(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_s2"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn eval_flag_writes_each_result() {
    let o = s2(&["-e", "'(a . (b c)) ''x #(1 2/4) (define x 10) (if (< x 20) (+ x 1) 0) (* 6 7)"], "");
    assert!(o.status.success());
    assert_eq!(stdout(&o), "(a b c)\n(quote x)\n#(1 1/2)\nx\n11\n42\n");
}

#[test]
fn stdin_without_a_terminal_has_no_prompt() {
    let o = s2(&[], "'#0=(1 . #0#)\n\"s\"\n");
    assert!(o.status.success());
    assert_eq!(stdout(&o), "#0=(1 . #0#)\n\"s\"\n");
}

#[test]
fn syntax_error_continues_and_fails() {
    let o = s2(&["-e", "1 #\\bogus 2 )"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "1\n2\n");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("unknown character name"), "{}", err);
    assert!(err.contains("Unexpected ')'"), "{}", err);
}

#[test]
fn runtime_error_continues_and_fails() {
    let o = s2(&["-e", "1 undefined-var (display \"after\")"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "1\nafter");
    assert!(String::from_utf8_lossy(&o.stderr).contains("unbound variable: undefined-var"));
}

#[test]
fn file_results_are_not_written() {
    let path = std::env::temp_dir().join(format!("s2-cli-test-{}.scm", std::process::id()));
    std::fs::write(&path, "(define x 6) (* x 7) (display (* x 7)) (newline)").unwrap();
    let o = s2(&[path.to_str().unwrap()], "");
    std::fs::remove_file(&path).ok();
    assert!(o.status.success());
    assert_eq!(stdout(&o), "42\n");
}

#[test]
fn dis_prints_code_without_running_it() {
    let o = s2(&["-e", "(dis '(display 1)) (dis '(if))"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "template top-level\n   0  GlobalRef k0     ; display\n   1  Fixnum 1\n   2  TailCall 1\n");
    assert!(String::from_utf8_lossy(&o.stderr).contains("if: bad syntax: (if)"));
}

#[test]
fn expand_prints_the_core_ast() {
    let o = s2(&["--expand", "-e", "(let ((x 1)) (and x 2))"], "");
    assert!(o.status.success());
    assert_eq!(stdout(&o), "(let ((x.1 1)) (if x.1 2 #f))\n2\n");
}

#[test]
fn bad_usage() {
    assert_eq!(s2(&["-x"], "").status.code(), Some(2));
}
