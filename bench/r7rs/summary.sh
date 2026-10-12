#!/usr/bin/env bash
# summary.sh RESULTS.CSV "SYSTEMS": a Markdown table of each benchmark's
# median time on each system, and each system's time relative to s2:
# the geometric mean, over the benchmarks both completed, of its time
# divided by s2's (below 1 is faster than s2).
awk -F, -v systems="$2" '
NR == 1 { next }
$3 == 0 { status[$1, $2] = $4; next }
{
    if (!($2 in seen)) { seen[$2] = 1; order[++nb] = $2 }
    if ($4 ~ /^[0-9.e+-]+$/) times[$1, $2] = times[$1, $2] " " $4
    else status[$1, $2] = $4
}
function median(list,    a, n, i, j, t) {
    n = split(list, a, " ")
    for (i = 2; i <= n; i++) for (j = i; j > 1 && a[j] + 0 < a[j - 1] + 0; j--) { t = a[j]; a[j] = a[j - 1]; a[j - 1] = t }
    return n % 2 ? a[(n + 1) / 2] : (a[n / 2] + a[n / 2 + 1]) / 2
}
END {
    ns = split(systems, sys, " ")
    printf "| benchmark |"; for (s = 1; s <= ns; s++) printf " %s |", sys[s]; print ""
    printf "|---|"; for (s = 1; s <= ns; s++) printf "---:|"; print ""
    for (b = 1; b <= nb; b++) {
        printf "| %s |", order[b]
        for (s = 1; s <= ns; s++) {
            k = sys[s] SUBSEP order[b]
            if (k in times) { m[k] = median(times[k]); printf " %.3f |", m[k] }
            else printf " %s |", (k in status ? status[k] : "-")
        }
        print ""
    }
    print ""
    print "| relative to s2 (geometric mean) | benchmarks |"
    print "|---|---:|"
    for (s = 1; s <= ns; s++) {
        sum = 0; n = 0
        for (b = 1; b <= nb; b++) {
            k = sys[s] SUBSEP order[b]; k2 = "s2" SUBSEP order[b]
            if ((k in m) && (k2 in m) && m[k] > 0 && m[k2] > 0) { sum += log(m[k] / m[k2]); n++ }
        }
        if (n) printf "| %s | %.3f | %d |\n", sys[s], exp(sum / n), n
    }
}' "$1"
