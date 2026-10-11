;; Tail calls: instruction dispatch.
(define count (lambda (n) (if (= n 0) 'done (count (- n 1)))))
(display (count 10000000))
