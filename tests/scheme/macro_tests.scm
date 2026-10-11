;; Ported from s1's scheme/macro_tests.scm: the one section that doesn't
;; test s1's procedural macro form (left out of s2 by design). s1's
;; hygiene tests are in syntax_rules_tests.scm.

(display "          === Testing Quasiquote at top level ===")
(newline)

;; Quasiquote works outside any macro context
(define **qq-a** 10)
(test-equal '(1 10 3) `(1 ,**qq-a** 3) "top-level unquote")
(test-equal '(1 2 3 4) `(1 ,@'(2 3) 4) "top-level splice")
(test-equal '(3 12) `(,(+ 1 2) ,(* 3 4)) "top-level computed unquotes")
(test-equal '(a b c) `(a b c) "top-level qq without unquotes")
(test-equal '(1 2 3) `(,@'(1 2 3)) "top-level all-splice")
(test-equal '(5 6 7 8) (let ((x 5) (y '(6 7))) `(,x ,@y 8)) "qq inside let")
(test-equal '() `() "qq of empty list")

