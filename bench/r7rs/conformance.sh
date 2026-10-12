#!/usr/bin/env bash
# Run chibi's R7RS suite (tests/r7rs) on s2 and other Schemes (plan/04 P12).
#
#   bench/r7rs/conformance.sh [SYSTEMS]
#
# As tests/r7rs/run.sh does for s2: the suite is split into sections, and
# each runs alone, as a program made of the suite's imports, shim.scm (in
# place of (chibi test)), the section and a summary call. Here every system
# runs it as a program, s2 too, so a top-level error ends a section on all
# of them alike; tests/r7rs/run.sh uses s2's `-f`, which carries on, so its
# counts are a little higher. Prints a Markdown table of tests passed per
# section, and the full output goes to target/r7rs-conformance/.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
tests="$root/tests/r7rs"
# Not Chez: it is an R6RS system, whose reader rejects R7RS syntax such as
# #u8(...), and chez/ has only what the benchmarks need. Not s7, which
# has no `import`.
systems="${1:-s2 s1 chibi guile gauche chicken gambit racket}"
S1="${S1:-$root/../s1/target/release/s1}"
out="$root/target/r7rs-conformance"
rm -rf "$out"
mkdir -p "$out"

if [[ " $systems " == *" s2 "* ]]; then
    (cd "$root" && cargo build --release -q) || exit 1
fi

run() {
    local sys=$1 f=$2
    case $sys in
        s2) "$root/target/release/s2" "$f" ;;
        s1) "$S1" -q "$f" ;;
        chibi) chibi-scheme "$f" ;;
        guile) guile --r7rs --no-auto-compile "$f" ;;
        gauche) gosh -r7 "$f" ;;
        chicken) csi -s "$f" ;;
        gambit) gsi "$f" ;;
        racket) { echo '#lang r7rs'; cat "$f"; } > "$f.rkt" && racket "$f.rkt" ;;
    esac
}

timeout_cmd=timeout
command -v timeout >/dev/null || timeout_cmd=gtimeout

awk '/^\(import /, /^ *\)$/' "$tests/r7rs-tests.scm" | grep -v '(chibi test)' > "$out/header.scm"
awk -v dir="$out" '
    /^\(test-begin "/ && !/^\(test-begin "R7RS"\)/ { n++; file = sprintf("%s/section-%02d.scm", dir, n) }
    n > 0 { print > file }
' "$tests/r7rs-tests.scm"

results="$out/results.tsv"
: > "$results"
for f in "$out"/section-*.scm; do
    name="$(sed -n '1s/^(test-begin "\(.*\)").*/\1/p' "$f")"
    for sys in $systems; do
        p="$out/$sys-$(basename "$f")"
        cat "$out/header.scm" "$tests/shim.scm" "$f" > "$p"
        echo '(%r7rs-summary)' >> "$p"
        output="$($timeout_cmd 60 bash -c "$(declare -f run); root='$root' here='$here' S1='$S1' run $sys '$p'" 2>&1 </dev/null)"
        printf '%s\n' "$output" > "$p.out"
        pass="$(printf '%s\n' "$output" | sed -n 's/^SUMMARY .* pass=\([0-9]*\).*/\1/p' | tail -n 1)"
        printf '%s\t%s\t%s\n' "$name" "$sys" "${pass:--}" >> "$results"
    done
done

awk -F'\t' -v systems="$systems" '
    { if (!($1 in seen)) { seen[$1] = 1; order[++n] = $1 } pass[$1, $2] = $3; if ($3 != "-") total[$2] += $3 }
    END {
        ns = split(systems, sys, " ")
        printf "| section |"; for (s = 1; s <= ns; s++) printf " %s |", sys[s]; print ""
        printf "|---|"; for (s = 1; s <= ns; s++) printf "---:|"; print ""
        for (i = 1; i <= n; i++) {
            printf "| %s |", order[i]
            for (s = 1; s <= ns; s++) printf " %s |", pass[order[i], sys[s]]
            print ""
        }
        printf "| **total** |"; for (s = 1; s <= ns; s++) printf " **%d** |", total[sys[s]]; print ""
    }' "$results" | tee "$out/summary.md"
