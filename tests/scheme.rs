//! s1's Scheme test files, ported (plan/05): each `tests/scheme/*_tests.scm`
//! runs after `harness.scm` and must report no failures and no errors.
//! Under `S2_GC_STRESS`, long loops and deep recursions are cut to 200
//! (`test-size`), since every call collects.

use std::path::Path;
use std::process::Command;

#[test]
fn scheme_test_files_pass() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scheme");
    let mut harness = std::fs::read_to_string(dir.join("harness.scm")).unwrap();
    if std::env::var_os("S2_GC_STRESS").is_some() {
        harness.push_str("(define (test-size n) (min n 200))\n");
    }
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    let mut failures = Vec::new();
    for scm in paths.iter().filter(|p| p.to_string_lossy().ends_with("_tests.scm")) {
        let program = format!("{}\n{}\n(test-summary)\n", harness, std::fs::read_to_string(scm).unwrap());
        let file = std::env::temp_dir().join(format!("s2-{}-{}", std::process::id(), scm.file_name().unwrap().to_string_lossy()));
        std::fs::write(&file, program).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_s2")).arg(&file).current_dir(env!("CARGO_MANIFEST_DIR")).output().unwrap();
        std::fs::remove_file(&file).ok();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !out.status.success() || stdout.contains("FAIL") || !stdout.contains(" 0 failed") {
            failures.push(format!("{}\n{}{}", scm.display(), stdout, stderr));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
