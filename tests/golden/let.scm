;; let variables are the stack slots their inits land in. In tail position
;; the frame is discarded anyway; elsewhere Slide removes them.
(define (f x) (let ((y (* x 2)) (z 3)) (+ x y z)))
(+ 1 (let ((a 2)) a) (let ((b 3)) b))
