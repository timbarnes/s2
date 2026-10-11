//! The s2 binary: read-print through `-e` and standard input, exit status.

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
fn eval_flag_writes_each_datum() {
    let o = s2(&["-e", "(a . (b c)) 'x #(1 2/4)"], "");
    assert!(o.status.success());
    assert_eq!(stdout(&o), "(a b c)\n(quote x)\n#(1 1/2)\n");
}

#[test]
fn stdin_without_a_terminal_has_no_prompt() {
    let o = s2(&[], "#0=(1 . #0#)\n\"s\"\n");
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
fn bad_usage() {
    assert_eq!(s2(&["-x"], "").status.code(), Some(2));
}
