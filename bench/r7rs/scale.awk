# Divide a benchmark input's first number, its iteration count, by SCALE
# (at least 1). Comment and blank lines are skipped.
!done && /^[ \t]*[0-9]/ {
    match($0, /[0-9]+/)
    n = int(substr($0, RSTART, RLENGTH) / SCALE)
    if (n < 1) n = 1
    $0 = substr($0, 1, RSTART - 1) n substr($0, RSTART + RLENGTH)
    done = 1
}
{ print }
