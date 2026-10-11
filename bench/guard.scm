;; Raising into guard: the exception machinery, written in Scheme.
(define (run n acc)
  (if (= n 0) acc
      (run (- n 1) (+ acc (guard (e ((symbol? e) 1)) (raise 'x))))))
(display (run 300000 0))
