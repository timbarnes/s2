//! Differential tests (plan/05 §3): each `tests/diff/*.scm` must print the
//! same thing on s2 as on s1, the reference implementation.
//!
//! s1 is found through `S1_BIN`, defaulting to `../s1/target/release/s1`.
//! If it can't be run, the test says so and passes, so `cargo test` works
//! without s1.

use std::path::Path;
use std::process::Command;

#[test]
fn same_output_as_s1() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let s1 = std::env::var("S1_BIN").unwrap_or_else(|_| root.join("../s1/target/release/s1").display().to_string());
    if !Command::new(&s1).arg("--help").output().is_ok_and(|o| o.status.success()) {
        eprintln!("skipping differential tests: can't run s1 at {} (set S1_BIN)", s1);
        return;
    }
    let mut paths: Vec<_> = std::fs::read_dir(root.join("tests/diff")).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let mut failures = Vec::new();
    for scm in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "scm")) {
        let a = Command::new(&s1).arg("-q").arg(scm).output().unwrap();
        let b = Command::new(env!("CARGO_BIN_EXE_s2")).arg(scm).output().unwrap();
        if a.stdout != b.stdout {
            failures.push(format!(
                "{}\n--- s1\n{}--- s2\n{}",
                scm.display(),
                String::from_utf8_lossy(&a.stdout),
                String::from_utf8_lossy(&b.stdout)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
