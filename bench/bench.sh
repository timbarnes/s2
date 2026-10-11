#!/usr/bin/env bash
# Benchmarks. Usage:
#   bench/bench.sh [reps]                    s1 against s2, every workload
#   bench/bench.sh --compare OLD NEW [reps]  two s2 binaries, every workload
# Each workload is a file in bench/. Runs of the two binaries alternate, so
# machine noise (large on a laptop) hits both alike; the table shows
# medians and their ratio. Only ratios should be compared across sessions.
# S1_BIN names the s1 binary (default ../s1/target/release/s1).
# ponytail: shell + /usr/bin/time, as in s1's bench.sh. A real harness when we
# need more than whole-program wall clock.
set -uo pipefail
cd "$(dirname "$0")/.."
if [[ "${1:-}" == "--compare" ]]; then
  A=(${2:?old binary}); B=(${3:?new binary}); REPS=${4:-7}; LA=old; LB=new
else
  REPS=${1:-7}
  cargo build --release >/dev/null 2>&1 || { echo "build failed"; exit 1; }
  A=(${S1_BIN:-../s1/target/release/s1} -q); B=(target/release/s2); LA=s1; LB=s2
fi
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT

median() { sort -n | awk '{a[NR]=$1} END{if(NR==0){print "n/a";exit} print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }
secs() { /usr/bin/time -p "$@" >/dev/null 2>"$W/t" </dev/null; awk '/^real/{print $2}' "$W/t"; }

printf '%-12s %8s %8s %8s\n' workload "$LA" "$LB" "$LA/$LB"
for f in bench/*.scm; do
  name=$(basename "$f" .scm)
  : > "$W/a"; : > "$W/b"
  for _ in $(seq "$REPS"); do
    secs "${A[@]}" "$f" >> "$W/a"
    secs "${B[@]}" "$f" >> "$W/b"
  done
  a=$(median < "$W/a"); b=$(median < "$W/b")
  printf '%-12s %7ss %7ss %7s\n' "$name" "$a" "$b" "$(awk -v a="$a" -v b="$b" 'BEGIN{if(b>0) printf "%.2fx", a/b; else print "-"}')"
done
