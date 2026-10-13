#!/usr/bin/env bash
# Build s2 without the Rust compiler (plan/08, P13.5) and run its tests
# and chibi's suite. It starts from stage-2 images: core.scm and the
# compiler, compiled by the compiler in Scheme in a default build.
#
#   tests/no-rust.sh

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

cargo build --release
images="$PWD/target/no-rust/release"
mkdir -p "$images"
S2_COMPILER=scheme target/release/s2 --core-image "$images/s2-core.s2o"
S2_COMPILER=scheme target/release/s2 --compiler-image "$images/s2-compiler.s2o"

cargo build --release --no-default-features --target-dir target/no-rust
# Test executables aren't next to the images: name them.
S2_CORE_IMAGE="$images/s2-core.s2o" S2_COMPILER_IMAGE="$images/s2-compiler.s2o" \
    cargo test --release --no-default-features --target-dir target/no-rust
S2_BIN="$images/s2" tests/r7rs/run.sh | tail -1
