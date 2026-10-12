#!/usr/bin/env bash
# Run ecraven's R7RS benchmarks (github.com/ecraven/r7rs-benchmarks) on s2,
# s1 and other Schemes, on this machine (plan/04 P12).
#
#   bench/r7rs/run.sh [-n ROUNDS] [-s SCALE] [-t SECONDS] [SYSTEMS [BENCHMARKS]]
#
#   -n ROUNDS   times each benchmark runs on each system (default 3); the
#               systems take turns, and the summary shows medians
#   -s SCALE    divide each benchmark's iteration count by SCALE (default
#               10; at least one iteration). The inputs are sized for about
#               a second on a native-code Scheme, a minute on s2
#   -t SECONDS  time limit per run (default 300)
#   SYSTEMS     "s2 s1 chibi ..." (default: all of them)
#   BENCHMARKS  "fib tak ..." (default: all of them)
#
# Each benchmark times itself, inside the process: startup and compiling
# are not counted. Results go to target/r7rs-results/<time>/: results.csv
# (system,benchmark,round,seconds or a status) and summary.md.
#
# The suite is cloned into target/r7rs-benchmarks at a fixed commit. Each
# benchmark is the suite's preparation of it: a system's prelude, the
# benchmark, common.scm, a postlude, common-postlude.scm. The preludes are
# the suite's, except where name.scm stands in for one. Chez runs on the
# R7RS libraries in chez/, which are just enough for these programs.

set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
suite="$root/target/r7rs-benchmarks"
suite_commit=85f6acdc4cc4e2b857f307ba56bd0ba931dcccd1

rounds=3 scale=10 limit=300
while getopts "n:s:t:" opt; do
    case $opt in
        n) rounds=$OPTARG ;;
        s) scale=$OPTARG ;;
        t) limit=$OPTARG ;;
        *) sed -n '2,16p' "$0" >&2; exit 2 ;;
    esac
done
shift $((OPTIND - 1))

all_systems="s2 s1 chibi guile gauche s7 chicken gambit chez racket"
# The suite's list (its `bench` script), in its order.
all_benchmarks="browse deriv destruc diviter divrec puzzle triangl tak takl ntakl cpstak ctak
    fib fibc fibfp sum sumfp fft mbrot mbrotZ nucleic pi pnpoly ray simplex
    ack array1 string sum1 cat tail wc read1
    compiler conform dynamic earley graphs lattice matrix maze mazefun nqueens paraffins parsing
    peval primes quicksort scheme slatex chudnovsky
    nboyer sboyer gcbench mperm equal bv2string"
systems="${1:-$all_systems}"
benchmarks="${2:-$all_benchmarks}"

if [[ ! -d "$suite" ]]; then
    git clone -q https://github.com/ecraven/r7rs-benchmarks.git "$suite" || exit 1
fi
git -C "$suite" checkout -q "$suite_commit" || exit 1
if [[ " $systems " == *" s2 "* ]]; then
    (cd "$root" && cargo build --release -q) || exit 1
fi

S2="$root/target/release/s2"
S1="${S1:-$root/../s1/target/release/s1}"
S7="${S7:-s7}"
CHEZ_LIBS="$here/chez"

out="$root/target/r7rs-results/$(date +%Y%m%d-%H%M%S)"
work="$out/work"
mkdir -p "$work"

# The prelude and postlude of each system, before and after the benchmark.
prelude() {
    case $1 in
        s2 | s1) echo "$here/name.scm" ;;
        chibi) echo "$suite/src/Chibi-prelude.scm" ;;
        guile) echo "$suite/src/Guile3-prelude.scm" ;;
        s7) echo "$suite/src/S7-prelude.scm" ;;
        chicken) echo "$suite/src/Chicken-prelude.scm" ;;
        gambit) echo "$suite/src/GambitC-prelude.scm" ;;
        racket) echo "$suite/src/Racket-prelude.scm" ;;
        *) echo /dev/null ;;
    esac
}
postlude() {
    case $1 in
        # The suite's Chez and Gauche ones need libraries not installed here.
        chez | gauche) echo "$here/name.scm" ;;
        chibi) echo "$suite/src/Chibi-postlude.scm" ;;
        *) echo /dev/null ;;
    esac
}

# Compile $1.scm ahead, if the system does; the program to run is then
# what `run` is given.
compile() {
    local sys=$1 f=$2
    case $sys in
        # Into Guile's cache, where `guile FILE` finds it.
        guile) guild compile -O3 "$f.scm" >/dev/null 2>&1 ;;
        chicken) csc -O3 -d0 "$f.scm" -o "$f.exe" >/dev/null 2>&1 ;;
        gambit) sed -i '' -e 's/^(import (scheme.*)$//' "$f.scm" && gsc -o "$f.exe" -exe "$f.scm" >/dev/null 2>&1 ;;
        racket) raco make "$f.scm" >/dev/null 2>&1 ;;
        *) true ;;
    esac
}

run() {
    local sys=$1 f=$2
    case $sys in
        s2) "$S2" "$f.scm" ;;
        s1) "$S1" -q "$f.scm" ;;
        chibi) chibi-scheme "$f.scm" ;;
        guile) GC_INITIAL_HEAP_SIZE=100000000 guile "$f.scm" ;;
        gauche) gosh -r7 -b "$f.scm" ;;
        s7) "$S7" "$f.scm" ;;
        chicken | gambit) "$f.exe" ;;
        chez) chez --optimize-level 2 --libdirs "$CHEZ_LIBS" --program "$f.scm" ;;
        racket) racket "$f.scm" ;;
    esac
}

timeout_cmd=timeout
command -v timeout >/dev/null || timeout_cmd=gtimeout

csv="$out/results.csv"
echo "system,benchmark,round,seconds" > "$csv"
cd "$suite"
for b in $benchmarks; do
    awk -v SCALE="$scale" -f "$here/scale.awk" "inputs/$b.input" > "$work/$b.input"
    for sys in $systems; do
        f="$work/$sys-$b"
        cat "$(prelude $sys)" "src/$b.scm" src/common.scm "$(postlude $sys)" src/common-postlude.scm > "$f.scm"
        compile "$sys" "$f" || echo "$sys,$b,0,COMPILEERROR" >> "$csv"
    done
    for ((round = 1; round <= rounds; round++)); do
        for sys in $systems; do
            f="$work/$sys-$b"
            grep -q "^$sys,$b,0,COMPILEERROR" "$csv" && continue
            # Each command runs in the suite's directory, which the
            # benchmarks' file names are relative to.
            $timeout_cmd "$limit" bash -c "$(declare -f run); work='$work' S1='$S1' S2='$S2' S7='$S7' CHEZ_LIBS='$CHEZ_LIBS' run $sys '$f'" \
                < "$work/$b.input" > "$f.out" 2>&1
            status=$?
            result=$(sed -n 's/^+!CSVLINE!+[^,]*,[^,]*,//p' "$f.out" | tail -n 1)
            if [[ $status -eq 124 ]]; then
                result=TIMEOUT
            elif [[ -z "$result" ]]; then
                result=ERROR
            fi
            echo "$sys,$b,$round,$result" >> "$csv"
            printf '%-12s %-8s %d  %s\n' "$b" "$sys" "$round" "$result"
        done
    done
done

"$here/summary.sh" "$csv" "$systems" > "$out/summary.md"
echo "Results: $out"
