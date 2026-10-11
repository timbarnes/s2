#!/usr/bin/env bash
# Run the vendored R7RS conformance suite (chibi-scheme's r7rs-tests.scm)
# against s2 and compare the pass counts with baseline.txt. Forked from s1.
#
#   tests/r7rs/run.sh             run, print the table, exit 1 if any
#                                 section passes fewer tests than the baseline
#   tests/r7rs/run.sh --update    run and overwrite baseline.txt
#
# The suite is split at each top-level (test-begin "...") and every section
# runs in its own s2 process, so a reader desync or crash in one section
# cannot swallow the sections after it. Full output goes to last-run.log.
#
# Set S2_BIN to test a prebuilt binary; otherwise a release build is made.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"

suite="$here/r7rs-tests.scm"
shim="$here/shim.scm"
baseline="$here/baseline.txt"
log="$here/last-run.log"
timeout_secs="${S2_TIMEOUT:-15}"  # per section; the whole suite takes ~1s

update=0
case "${1:-}" in
    --update) update=1 ;;
    "") ;;
    *) echo "usage: $0 [--update]" >&2; exit 2 ;;
esac

if [[ -z "${S2_BIN:-}" ]]; then
    if ! build_log="$(cargo build --release 2>&1)"; then
        printf '%s\n' "$build_log" >&2
        exit 1
    fi
    S2_BIN="$root/target/release/s2"
fi

# GNU timeout, or gtimeout (Homebrew coreutils) on macOS; without either,
# sections run with no time limit.
if command -v timeout >/dev/null; then
    with_timeout() { timeout "$timeout_secs" "$@"; }
elif command -v gtimeout >/dev/null; then
    with_timeout() { gtimeout "$timeout_secs" "$@"; }
else
    with_timeout() { "$@"; }
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
echo '(%r7rs-summary)' > "$work/summary.scm"

# ponytail: the suite's (import ...) header is not run: s2 has no libraries
# until P8, when it comes back (as in s1's runner).

# Split into one file per section. The outer (test-begin "R7RS") is dropped:
# totals are summed here instead.
awk -v dir="$work" '
    /^\(test-begin "/ && !/^\(test-begin "R7RS"\)/ {
        n++
        file = sprintf("%s/section-%02d.scm", dir, n)
    }
    n > 0 { print > file }
' "$suite"

# Approximate number of tests a section contains, counted statically:
# test-numeric-syntax expands to two tests, the other helpers to one.
# Helper definitions inside define-syntax templates are counted too, so this
# is an estimate, used only to show how much of a section was never reached.
expected_tests() {
    awk '
        # Skip #| ... |# block comments (the suite has commented-out tests).
        /^[ \t]*#\|/ { in_block = 1 }
        in_block { if ($0 ~ /\|#/) in_block = 0; next }
        /^[ \t]*;/ { next }
        /^[ \t]*\(test-numeric-syntax[ \t]/ { n += 2; next }
        /^[ \t]*\(test(-assert|-values|-error|-write-syntax|-precision|-read-error)?[ \t)]/ { n++ }
        END { print n + 0 }
    ' "$1"
}

: > "$log"
results="$work/results.txt"
: > "$results"

for f in "$work"/section-*.scm; do
    name="$(sed -n '1s/^(test-begin "\(.*\)").*/\1/p' "$f")"
    expected="$(expected_tests "$f")"
    echo ";;;; ==== $name" >> "$log"
    status=0
    # s2 runs one file, so the shim, the section and the summary call are
    # put together. s2 exits 1 if any form had an error, which is expected.
    cat "$shim" "$f" "$work/summary.scm" > "$work/run.scm"
    out="$(with_timeout "$S2_BIN" "$work/run.scm" 2>&1 </dev/null)" || status=$?
    printf '%s\n' "$out" >> "$log"

    summary="$(printf '%s\n' "$out" | grep '^SUMMARY ' | tail -n 1 || true)"
    note=""
    if [[ $status -eq 124 ]]; then
        note="TIMEOUT"
    elif [[ -z "$summary" ]]; then
        note="CRASH(exit $status)"
    fi
    field() { printf '%s\n' "$summary" | sed -n "s/.* $1=\([0-9]*\).*/\1/p"; }
    attempted="$(field attempted)"; pass="$(field pass)"
    fail="$(field fail)"; error="$(field error)"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$name" "${pass:-0}" "${fail:-0}" "${error:-0}" "${attempted:-0}" \
        "$expected" "$note" >> "$results"
done

table() {
    awk -F'\t' '
        BEGIN {
            fmt = "%-34s %5s %5s %5s %5s %6s  %s\n"
            printf fmt, "section", "pass", "fail", "error", "unrch", "~total", ""
        }
        {
            unreached = $6 - $5; if (unreached < 0) unreached = 0
            printf fmt, $1, $2, $3, $4, unreached, $6, $7
            p += $2; f += $3; e += $4; u += unreached; t += $6
        }
        END { printf fmt, "TOTAL", p, f, e, u, t, "" }
    ' "$results"
}

table

if [[ $update -eq 1 ]]; then
    {
        echo "# R7RS conformance baseline for s2 -- regenerate with tests/r7rs/run.sh --update"
        echo "# Columns: section, pass, fail, error, attempted, ~total (static estimate), note"
        cat "$results"
    } > "$baseline"
    echo
    echo "Baseline written to ${baseline#$root/}"
    exit 0
fi

if [[ ! -f "$baseline" ]]; then
    echo
    echo "No baseline yet; run with --update to create one."
    exit 0
fi

# Compare pass counts section by section.
echo
awk -F'\t' '
    NR == FNR { if ($0 !~ /^#/) base[$1] = $2; next }
    {
        seen[$1] = 1
        if (!($1 in base)) { printf "NEW SECTION  %s: %d passing\n", $1, $2; next }
        total_now += $2; total_base += base[$1]
        if ($2 < base[$1]) {
            printf "REGRESSION   %s: %d -> %d passing\n", $1, base[$1], $2; bad = 1
        } else if ($2 > base[$1]) {
            printf "improved     %s: %d -> %d passing\n", $1, base[$1], $2
        }
    }
    END {
        for (s in base) if (!(s in seen)) { printf "MISSING      %s\n", s; bad = 1 }
        printf "Passing: %d (baseline %d)\n", total_now, total_base
        exit bad
    }
' "$baseline" "$results"
