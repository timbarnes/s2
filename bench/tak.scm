;; Takeuchi: calls with three arguments and fixnum comparisons.
(define (tak x y z) (if (not (< y x)) z (tak (tak (- x 1) y z) (tak (- y 1) z x) (tak (- z 1) x y))))
(define (run n acc) (if (= n 0) acc (run (- n 1) (tak 18 12 6))))
(display (run 100 0))
