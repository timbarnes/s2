;;; (scheme time) for Chez Scheme: on Chez's clocks
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme time)
  (export
    current-jiffy current-second jiffies-per-second)
  (import (chezscheme))

  (define (current-jiffy)
    (let ((t (current-time 'time-monotonic)))
      (+ (* (time-second t) 1000000000) (time-nanosecond t))))
  (define (jiffies-per-second) 1000000000)
  (define (current-second)
    (let ((t (current-time 'time-utc)))
      (+ (time-second t) (/ (time-nanosecond t) 1e9))))
)
