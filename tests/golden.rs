//! Golden disassembly tests (plan/05 §2): for each `tests/golden/NAME.scm`,
//! the output of `s2 --dis NAME.scm` must equal `NAME.dis`.
//!
//! After a deliberate change to code generation, regenerate the expected
//! files with `UPDATE_GOLDEN=1 cargo test --test golden` and review the
//! diff.

use std::path::Path;
use std::process::Command;

#[test]
fn golden_disassembly() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for scm in paths.iter().filter(|p| p.extension().is_some_and(|e| e == "scm")) {
        let out = Command::new(env!("CARGO_BIN_EXE_s2")).arg("--dis").arg(scm).output().unwrap();
        let actual = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        let expected_path = scm.with_extension("dis");
        if update {
            std::fs::write(&expected_path, &actual).unwrap();
        } else if std::fs::read_to_string(&expected_path).ok().as_deref() != Some(actual.as_str()) {
            failures.push(format!("{}:\n{}", scm.display(), actual));
        }
    }
    assert!(failures.is_empty(), "golden mismatches (UPDATE_GOLDEN=1 to accept):\n{}", failures.join("\n"));
}
