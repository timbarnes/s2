;; if: JumpIfFalse skips the then-branch, Jump skips the else-branch.
(define x 10)
(if (< x 20) (+ x 1) 0)
(if #f #f)
