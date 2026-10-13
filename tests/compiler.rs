//! The compiler oracle (plan/08, C7): every file of a corpus is compiled
//! with each compiler `S2_COMPILER` can select, with `--expand --dis`, and
//! the output must be the same as the Rust compiler's, line for line.
//!
//! The corpus is what this repository has:
//!
//! - compiled only (`--compile`): the golden files, `scheme/core.scm` and
//!   `bench/`, whose programs take a while to run;
//! - run (`-f`): the Scheme test files (after their harness) and each
//!   section of chibi's R7RS suite (after its import header and shim, as
//!   `tests/r7rs/run.sh` runs them). Running them also compiles what they
//!   compile as they go (`eval`, `load`, library files), and gets past what
//!   compiling ahead can't do: a circular constant, a file a test writes
//!   and then includes.
//!
//! Also here: compiling a file gives the same bytes every time, however
//! the heap was used (P13.0).

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sorted(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(suffix)).collect();
    paths.sort();
    paths
}

/// The corpus, as (name, program text, whether to run it).
fn corpus() -> Vec<(String, String, bool)> {
    let root = root();
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    let mut files = Vec::new();
    for p in sorted(&root.join("tests/golden"), ".scm").into_iter().chain(sorted(&root.join("bench"), ".scm")) {
        files.push((p.display().to_string(), read(&p), false));
    }
    files.push(("scheme/core.scm".into(), read(&root.join("scheme/core.scm")), false));
    let harness = read(&root.join("tests/scheme/harness.scm"));
    for p in sorted(&root.join("tests/scheme"), "_tests.scm") {
        files.push((p.display().to_string(), format!("{}\n{}\n(test-summary)\n", harness, read(&p)), true));
    }
    // chibi's suite, split into its sections as run.sh does.
    let suite = read(&root.join("tests/r7rs/r7rs-tests.scm"));
    let shim = read(&root.join("tests/r7rs/shim.scm"));
    let mut header = String::new();
    let mut in_import = false;
    for line in suite.lines() {
        if line.starts_with("(import ") {
            in_import = true;
        }
        if in_import && !line.contains("(chibi test)") {
            header.push_str(line);
            header.push('\n');
        }
        if in_import && line.trim_start().starts_with(')') && line.trim() == ")" {
            in_import = false;
        }
    }
    let mut section: Option<String> = None;
    let mut n = 0;
    for line in suite.lines() {
        if line.starts_with("(test-begin \"") && !line.starts_with("(test-begin \"R7RS\")") {
            if let Some(text) = section.take() {
                n += 1;
                files.push((format!("r7rs section {}", n), text, true));
            }
            section = Some(format!("{}\n{}\n", header, shim));
        }
        if let Some(text) = &mut section {
            text.push_str(line);
            text.push('\n');
        }
    }
    if let Some(text) = section {
        files.push((format!("r7rs section {}", n + 1), text, true));
    }
    files
}

/// Compile `text` with the compiler `compiler`, with `--expand --dis`, or
/// run it if `run`, and return what it printed and its status, as text,
/// and the compiled file.
fn compile(test: &str, compiler: &str, name: &str, text: &str, run: bool, env: &[(&str, &str)]) -> (String, Vec<u8>) {
    let tag = name.replace(['/', ' '], "_");
    let dir = std::env::temp_dir();
    // The tests run in parallel in one process, so each names its files
    // (`test`). A compiled file records its source's path, so compiling one
    // file twice uses the same path.
    let src = dir.join(format!("s2-oracle-{}-{}-{}-{}.scm", std::process::id(), test, compiler, tag));
    let out = src.with_extension("s2o");
    std::fs::write(&src, text).unwrap();
    // A compiled file records its source's modification time: the same
    // each time, so two compiles can be compared byte for byte.
    let epoch = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options().write(true).open(&src).unwrap().set_modified(epoch).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_s2"));
    command.args(["--expand", "--dis"]);
    if run {
        command.arg("-f").arg(&src);
    } else {
        command.arg("--compile").arg(&src).arg("-o").arg(&out);
    }
    let result = command
        .env("S2_COMPILER", compiler)
        .envs(env.iter().copied())
        .current_dir(root())
        .output()
        .unwrap();
    let bytes = std::fs::read(&out).unwrap_or_default();
    std::fs::remove_file(&src).ok();
    std::fs::remove_file(&out).ok();
    // The temporary file's name appears in error messages; take it out.
    let printed = format!("{}{}status {:?}\n", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr), result.status.code());
    (printed.replace(&src.display().to_string(), "FILE"), bytes)
}

/// The first line where `a` and `b` differ, with some context.
fn first_difference(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let from = i.saturating_sub(5);
    format!(
        "at line {}:\n--- rust\n{}\n--- other\n{}",
        i + 1,
        a[from..(i + 3).min(a.len())].join("\n"),
        b[from..(i + 3).min(b.len())].join("\n")
    )
}

/// Every corpus file, compiled by the Rust compiler and by `compiler`,
/// prints the same.
fn compiles_as_the_rust_compiler_does(compiler: &str) {
    let mut failures = Vec::new();
    for (name, text, run) in corpus() {
        let (rust, _) = compile(compiler, "rust", &name, &text, run, &[]);
        let (other, _) = compile(compiler, compiler, &name, &text, run, &[]);
        if rust != other {
            failures.push(format!("{}: {}", name, first_difference(&rust, &other)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn the_round_trip_compiles_as_the_rust_compiler_does() {
    compiles_as_the_rust_compiler_does("roundtrip");
}

#[test]
fn the_scheme_compiler_compiles_as_the_rust_compiler_does() {
    compiles_as_the_rust_compiler_does("scheme");
}

/// A library with what a compiled file must record: variables, a macro,
/// and the hidden definitions of several records, which share a name.
const LIBRARY: &str = "
(define-library (oracle shapes)
  (export make-point point-x make-size size-w twice)
  (import (scheme base))
  (begin
    (define-record-type point (make-point x y) point? (x point-x) (y point-y))
    (define-record-type size (make-size w h) size? (w size-w) (h size-h))
    (define scale 2)
    (define-syntax twice (syntax-rules () ((_ e) (* scale e))))))
(import (oracle shapes))
(define p (make-point 1 2))
(display (twice (+ (point-x p) (size-w (make-size 3 4)))))
";

#[test]
fn compiled_files_are_reproducible() {
    for (name, text) in [("library", LIBRARY.to_string()), ("core", std::fs::read_to_string(root().join("scheme/core.scm")).unwrap())] {
        let (_, plain) = compile("repro", "rust", name, &text, false, &[]);
        let (_, stressed) = compile("repro", "rust", name, &text, false, &[("S2_GC_STRESS", "1"), ("S2_GC_POISON", "1")]);
        assert!(!plain.is_empty(), "{}: nothing written", name);
        assert!(plain == stressed, "{}: compiling twice gave different bytes", name);
    }
}

/// Self-compilation (P13.5): the compiler compiled by the Rust compiler
/// (stage 1) compiles itself (stage 2), and that compiles itself again
/// (stage 3). All three are the same bytes. So is `core.scm`'s image,
/// compiled by stage 1 and by the Rust compiler.
#[test]
fn the_scheme_compiler_compiles_itself_to_a_fixed_point() {
    let dir = std::env::temp_dir();
    let file = |name: &str| dir.join(format!("s2-{}-{}.s2o", name, std::process::id()));
    let build = |flag: &str, out: &str, compiler: &str, image: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_s2"));
        command.arg(flag).arg(file(out)).env("S2_COMPILER", compiler);
        if let Some(image) = image {
            command.env("S2_COMPILER_IMAGE", file(image));
        }
        let result = command.output().unwrap();
        assert!(result.status.success(), "{}: {}", out, String::from_utf8_lossy(&result.stderr));
        let bytes = std::fs::read(file(out)).unwrap();
        std::fs::remove_file(file(out)).ok();
        bytes
    };
    let stage1 = build("--compiler-image", "stage-1", "rust", None);
    std::fs::write(file("stage-1-image"), &stage1).unwrap();
    let stage2 = build("--compiler-image", "stage-2", "scheme", Some("stage-1-image"));
    std::fs::write(file("stage-2-image"), &stage2).unwrap();
    let stage3 = build("--compiler-image", "stage-3", "scheme", Some("stage-2-image"));
    let core_rust = build("--core-image", "core-rust", "rust", None);
    let core_scheme = build("--core-image", "core-scheme", "scheme", Some("stage-1-image"));
    ["stage-1-image", "stage-2-image"].iter().for_each(|f| std::fs::remove_file(file(f)).unwrap());
    assert!(stage1 == stage2, "stage 2 differs from stage 1");
    assert!(stage2 == stage3, "stage 3 differs from stage 2");
    assert!(core_rust == core_scheme, "core.scm compiles differently");
}
