;; Tail positions: if branches, the last expression of a body or let.
;; Neither the call to (- n 1) nor the call to (= n 0) is in tail position.
(define (loop n acc) (if (= n 0) acc (loop (- n 1) (+ acc 1))))
(define (f g) (begin (g 1) (let ((x 2)) (g x))))
