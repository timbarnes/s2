;; Macros are gone by the time code is generated: my-or's expansion is a
;; let and an if, and its recursive use is a tail call.
(define-syntax my-or
  (syntax-rules () ((_) #f) ((_ e) e) ((_ e1 e2 ...) (let ((temp e1)) (if temp temp (my-or e2 ...))))))
(define (f x) (my-or x (car x)))
