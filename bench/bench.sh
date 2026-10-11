#!/usr/bin/env bash
# Benchmarks s2 against s1. Usage: bench/bench.sh [reps]
# Reports the median wall-clock time of each workload on each system.
# S1_BIN names the s1 binary (default ../s1/target/release/s1).
# ponytail: shell + /usr/bin/time, as in s1's bench.sh. A real harness when
# we need more than whole-program wall clock.
set -uo pipefail
cd "$(dirname "$0")/.."
REPS=${1:-5}
S1=${S1_BIN:-../s1/target/release/s1}
S2=target/release/s2
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT

cargo build --release >/dev/null 2>&1 || { echo "build failed"; exit 1; }

median() { sort -n | awk '{a[NR]=$1} END{if(NR==0){print "n/a";exit} print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }

secs() { /usr/bin/time -p "$@" >/dev/null 2>"$W/t" </dev/null; awk '/^real/{print $2}' "$W/t"; }

# The two systems are run alternately, so machine noise (which is large on
# a laptop) hits both alike, and the ratio is meaningful even when the
# absolute times drift.
for n in 25 30; do
  cat > "$W/fib$n.scm" <<SCM
(define fib (lambda (n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))
(display (fib $n)) (newline)
SCM
  : > "$W/a"; : > "$W/b"
  for _ in $(seq "$REPS"); do
    secs "$S1" -q "$W/fib$n.scm" >> "$W/a"
    secs "$S2" "$W/fib$n.scm" >> "$W/b"
  done
  a=$(median < "$W/a"); b=$(median < "$W/b")
  printf 'fib %-3s  s1 %5ss   s2 %5ss   s1/s2 %s\n' "$n" "$a" "$b" "$(awk -v a="$a" -v b="$b" 'BEGIN{printf "%.1fx", a/b}')"
done
