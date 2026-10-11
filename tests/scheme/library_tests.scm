;; library_tests.scm: environments, eval and libraries. Ported from s1's
;; scheme/library_tests.scm (see its design/libraries-design.md), with these
;; changes:
;; - errors s2 finds while compiling a top-level form (set! of an import,
;;   import or define-library in a body, a misplaced cond-expand else)
;;   can't be caught by a guard in that same form, so those tests compile
;;   the form with eval, where the error is raised;
;; - s1's extensions (one-argument eval, type-of, empty?, top) are left
;;   out or replaced, and (s1) is (s2);
;; - temporary files go in target/.

(display "          === Testing environments and eval ===")
(newline)

(test-equal 21 (eval '(* 7 3) (interaction-environment)) "eval in the interaction environment")
(test-equal 'a (eval ''a (interaction-environment)) "eval evaluates its expression once")
(test-equal '(x "s") (eval '(list 'x "s") (interaction-environment)) "eval of a call")
(test-equal 'unbound
    (let ((local-x 1)) (guard (e (#t 'unbound)) (eval 'local-x (interaction-environment))))
    "the interaction environment doesn't see the caller's locals")
(test-equal "eval: the second argument must be an environment"
    (guard (e (#t (error-object-message e))) (eval '(* 7 3) 'not-an-environment))
    "eval rejects a non-environment")
(test-equal "#<environment>"
    (let ((out (open-output-string)))
      (write (interaction-environment) out)
      (get-output-string out))
    "environments print opaquely")

;; define through eval binds in that environment, and the caller's
;; environment is restored afterwards, also after an error.
(test-equal '(5 42)
    (let ((y 5))
      (eval '(define eval-defined 42) (interaction-environment))
      (list y eval-defined))
    "eval's define binds at top level and the caller's environment is restored")
(test-equal 6
    (let ((y 6)) (guard (e (#t y)) (eval '(car 1) (interaction-environment))))
    "an error inside eval leaves the caller's environment intact")
(test-equal 7
    (let ((k-result (call/cc (lambda (k) (eval `(,k 7) (interaction-environment)))))) k-result)
    "a continuation called from inside eval")

;; load with an environment runs the file before returning
(define library-test-file "target/s2-library-test.tmp")
(call-with-output-file library-test-file
  (lambda (p) (write '(define loaded-value (* 6 7)) p) (write '(define loaded-twice (* 2 loaded-value)) p)))
(test-equal '(42 84)
    (let ((unused 0))
      (load library-test-file (interaction-environment))
      (list loaded-value loaded-twice))
    "load with an environment evaluates the file before returning")
(delete-file library-test-file)

(display "          === Testing standard libraries and environment ===")
(newline)

(test-equal 3 (eval '(+ 1 2) (environment '(scheme base))) "eval in an environment of (scheme base)")
(test-equal #\A (eval '(char-upcase #\a) (environment '(scheme base) '(scheme char)))
    "an environment of two libraries")
(test-equal 'unbound
    (guard (e (#t 'unbound)) (eval '(char-upcase #\a) (environment '(scheme base))))
    "an environment holds only what its libraries export")
(test-equal 'unbound
    (guard (e (#t 'unbound)) (eval '(%frame-depth) (environment '(scheme base))))
    "s2's extensions aren't in (scheme base)")
(test-equal #t (procedure? (eval 'dis (environment '(s2)))) "(s2) exports s2's extensions")
(test-equal "define: this environment is immutable"
    (guard (e (#t (error-object-message e))) (eval '(define x 1) (environment '(scheme base))))
    "an environment from environment is immutable")
(test-equal "set!: car is imported and can't be assigned"
    (guard (e (#t (error-object-message e))) (eval '(set! car cdr) (environment '(scheme base))))
    "set! in an environment from environment")
(test-equal 6 (eval '(let () (define x 6) x) (environment '(scheme base)))
    "local definitions in an immutable environment")
(test-equal "environment: unknown library (no such library)"
    (guard (e (#t (error-object-message e))) (environment '(no such library)))
    "an unknown library")
(test-equal '(case-lambda) (library-exports '(scheme case-lambda)) "library-exports")
(test-equal '(#t #t) (list (and (member '(scheme base) (library-names)) #t) (and (member '(s2) (library-names)) #t))
    "library-names")

;; What R7RS assigns the standard libraries that s2 doesn't define: the
;; auxiliary syntax and syntax-error (P9), and the complex-number
;; procedures, which aren't planned.
(test-equal '(((scheme base) ("..." "=>" "_" "else" "syntax-error"))
              ((scheme complex) ("angle" "imag-part" "magnitude" "make-polar" "make-rectangular" "real-part"))
              ((scheme r5rs) ("angle" "imag-part" "magnitude" "make-polar" "make-rectangular" "real-part")))
    (let loop ((names (library-names)) (acc '()))
      (cond ((null? names) (reverse acc))
            ((null? (%library-unimplemented (car names))) (loop (cdr names) acc))
            (else (loop (cdr names) (cons (list (car names) (%library-unimplemented (car names))) acc)))))
    "standard names s2 doesn't define yet")

(display "          === Testing the interaction environment's imports ===")
(newline)

;; The REPL's names are imported from the system: defining one shadows it
;; without changing the system's, which core.scm's procedures keep using.
(define saved-equal? equal?)
(define (equal? a b) 'shadowed)
(test-equal '(shadowed (2)) (list (equal? 1 1) (member 2 '(1 2)))
    "a REPL define shadows a built-in without affecting core.scm")
(define equal? saved-equal?)
(test-equal "set!: car is imported and can't be assigned"
    (guard (e (#t (error-object-message e))) (eval '(set! car cdr) (interaction-environment)))
    "set! of an imported name is an error")
(test-equal 1 (car '(1 2)) "and leaves it unchanged")
(define repl-defined 1)
(set! repl-defined 2)
(test-equal 2 repl-defined "set! of a REPL definition")
(test-equal 'outer
    (let ((car (lambda (x) 'outer))) (set! car (lambda (x) 'outer)) (car 1))
    "set! of a local that shadows an import")

(display "          === Testing import ===")
(newline)

(import (prefix (scheme char) c:))
(test-equal #\A (c:char-upcase #\a) "import with prefix")
(import (rename (only (scheme base) car cdr) (car first) (cdr rest-of)))
(test-equal '(1 (2)) (list (first '(1 2)) (rest-of '(1 2))) "import with only and rename")
(test-equal "set!: first is imported and can't be assigned"
    (guard (e (#t (error-object-message e))) (eval '(set! first cdr) (interaction-environment)))
    "a name bound by import can't be assigned")
(test-equal 'unbound
    (guard (e (#t 'unbound)) (eval '(car '(1)) (environment '(except (scheme base) car))))
    "environment with except")
(test-equal '(1 #\B)
    (eval '(list (head '(1)) (up #\b))
          (environment '(rename (only (scheme base) car list quote) (car head))
                       '(prefix (only (scheme char) char-upcase) x-)
                       '(rename (prefix (only (scheme char) char-upcase) x-) (x-char-upcase up))))
    "environment with combined import sets")
(test-equal "environment: no-such is not in the import set (scheme base)"
    (guard (e (#t (error-object-message e))) (environment '(only (scheme base) car no-such)))
    "only of a missing name is an error")
(test-equal "import: unknown library (no such)"
    (guard (e (#t (error-object-message e)))
      (eval '(import (scheme base) (no such)) (interaction-environment)))
    "importing an unknown library is an error")
(test-equal "import: only allowed at top level"
    (guard (e (#t (error-object-message e))) (eval '(let () (import (scheme base)) 1) (interaction-environment)))
    "import isn't allowed in a body")
(test-equal "import: this environment is immutable"
    (guard (e (#t (error-object-message e))) (eval '(import (scheme char)) (environment '(scheme base))))
    "import into an immutable environment")
;; A REPL definition can be replaced by an import, and vice versa.
(define local-then-imported 'local)
(import (rename (only (scheme base) car) (car local-then-imported)))
(test-equal 1 (local-then-imported '(1)) "import replaces a definition")
(define local-then-imported 'local-again)
(test-equal 'local-again local-then-imported "a definition replaces an import")

(test-equal 21 (eval '(* 7 3) (scheme-report-environment 5)) "scheme-report-environment")
(test-equal '(1 unbound)
    (list (eval '(if #t 1 2) (null-environment 5))
          (guard (e (#t 'unbound)) (eval '(car '(1)) (null-environment 5))))
    "null-environment has only syntax")
(test-equal "null-environment: the only version supported is 5"
    (guard (e (#t (error-object-message e))) (null-environment 7))
    "R5RS environments of other versions")

(display "          === Testing define-library ===")
(newline)

(define-library (test counter)
  (export count inc! (rename get-count current) bump!)
  (import (scheme base))
  (begin
    (define count 0)
    (define (inc!) (set! count (+ count 1)) count)
    (define (get-count) count)
    ;; An exported macro that assigns the library's own variable
    (define-syntax bump! (syntax-rules () ((_) (set! count (+ count 10)))))))
(import (test counter))
(inc!)
(inc!)
(test-equal '(2 2) (list count (current)) "an imported variable sees the library's assignments")
(bump!)
(test-equal 12 count "an exported macro can assign the library's variable")
(test-equal "set!: count is imported and can't be assigned"
    (guard (e (#t (error-object-message e))) (eval '(set! count 0) (interaction-environment)))
    "an importer can't assign a library's variable")
(define (get-count) 'importer)
(test-equal '(importer 12) (list (get-count) (current))
    "the importer's own definitions don't touch the library")
(test-equal '(count inc! current bump!) (library-exports '(test counter)) "library-exports of a defined library")

(define-library (test helpers)
  (export twice)
  (import (scheme base))
  (begin
    (define (helper x) (* 2 x))
    (define-syntax twice (syntax-rules () ((_ x) (helper x))))))
(import (test helpers))
(test-equal 6 (twice 3) "an exported macro can use the library's unexported helper")
(test-equal 'unbound (guard (e (#t 'unbound)) helper) "the helper itself isn't imported")

(define-library (test isolated)
  (export shout)
  (import (scheme base))
  (begin (define (shout c) (char-upcase c))))
(import (test isolated))
(test-equal "unbound variable: char-upcase"
    (guard (e (#t (error-object-message e))) (shout #\a))
    "a library sees only what it imports")

(define-library (test uses-counter)
  (export counter-plus)
  (import (scheme base) (only (test counter) count))
  (begin (define (counter-plus n) (+ count n))))
(import (test uses-counter))
(test-equal 13 (counter-plus 1) "a library can import another library")

(test-equal "define-library: (test broken) exports missing, which it doesn't define"
    (guard (e (#t (error-object-message e)))
      (eval '(define-library (test broken) (export missing) (import (scheme base)) (begin (define present 1)))
            (interaction-environment)))
    "an export that is never defined is an error")
(test-equal "import: unknown library (test broken)"
    (guard (e (#t (error-object-message e))) (eval '(import (test broken)) (interaction-environment)))
    "a library with an error isn't registered")
(test-equal 'failed
    (guard (e (#t 'failed))
      (eval '(define-library (test failing) (export x) (import (scheme base)) (begin (define x (car '()))))
            (interaction-environment)))
    "a failing body raises")
(test-equal #f (and (member '(test failing) (library-names)) #t) "and the library isn't registered")
(test-equal "define-library: x is imported with two different bindings"
    (guard (e (#t (error-object-message e)))
      (eval '(define-library (test clash) (export y) (import (scheme base) (rename (only (scheme base) car) (car x)) (rename (only (scheme base) cdr) (cdr x))) (begin (define y 1)))
            (interaction-environment)))
    "importing one name twice differently is an error")
(test-equal "define-library: only allowed at top level"
    (guard (e (#t (error-object-message e)))
      (eval '(let () (define-library (test inner) (export) (import (scheme base))) 1) (interaction-environment)))
    "define-library isn't allowed in a body")

;; include, include-ci and include-library-declarations
(define include-test-file "target/s2-include-test.tmp")
(define include-ci-file "target/s2-include-ci.tmp")
(define include-decl-file "target/s2-include-decl.tmp")
(call-with-output-file include-test-file
  (lambda (p) (write '(define (included-double x) (* 2 x)) p)))
(call-with-output-file include-ci-file
  (lambda (p) (display "(DEFINE INCLUDED-UPPER 'Up)" p)))
(call-with-output-file include-decl-file
  (lambda (p) (write '(export included-double) p) (write '(import (scheme base)) p)))
(define-library (test included)
  (include-library-declarations "target/s2-include-decl.tmp")
  (include "target/s2-include-test.tmp"))
(import (test included))
(test-equal 8 (included-double 4) "include and include-library-declarations in a library")
(define-library (test included-ci)
  (export included-upper)
  (import (scheme base))
  (include-ci "target/s2-include-ci.tmp"))
(import (test included-ci))
(test-equal 'up included-upper "include-ci folds case")
(test-equal 10 (let () (include "target/s2-include-test.tmp") (included-double 5)) "include in a body")
(delete-file include-test-file)
(delete-file include-ci-file)
(delete-file include-decl-file)

(display "          === Testing library files ===")
(newline)

;; The fixtures are in tests/libs, found through the current directory.
(import (tests libs uses-greet))
(test-equal '("hello, a" "hello, a") (greet-twice "a")
    "a library file found on the search path, importing another")
(import (tests libs greet))
(test-equal "hello, b" (greet "b") "the library it imported was loaded too")
(test-equal #t (and (member '(tests libs greet) (library-names)) #t) "and registered")

;; A file loaded without defining its library is an error, and isn't loaded
;; again. (The count survives this file being run twice, under GC stress.)
(define sld-load-count (guard (e (#t 0)) sld-load-count))
(define wrong-name-error
  (cond-expand
    (windows "import: .\\tests\\libs\\wrong-name.sld does not define (tests libs wrong-name)")
    (else "import: ./tests/libs/wrong-name.sld does not define (tests libs wrong-name)")))
(test-equal wrong-name-error
    (guard (e (#t (error-object-message e))) (eval '(import (tests libs wrong-name)) (interaction-environment)))
    "a library file that doesn't define its library")
(test-equal wrong-name-error
    (guard (e (#t (error-object-message e))) (eval '(import (tests libs wrong-name)) (interaction-environment)))
    "the same error again")
(test-equal 1 sld-load-count "the file was loaded once")
(test-equal "import: unknown library (tests libs no-such-file)"
    (guard (e (#t (error-object-message e))) (eval '(import (tests libs no-such-file)) (interaction-environment)))
    "a library with no file")

(display "          === Testing cond-expand and features ===")
(newline)

(test-equal '(#t #t #t #t) (map (lambda (f) (and (memq f (features)) #t)) '(r7rs ratios s2 full-unicode)) "features")
(test-equal #f (and (memq 'exact-complex (features)) #t) "no complex numbers")
(test-equal 'yes (cond-expand (r7rs 'yes) (else 'no)) "a feature")
(test-equal 'else (cond-expand (no-such-feature 'yes) (else 'else)) "else")
(test-equal 'and-or-not
    (cond-expand ((and s2 (or no-such-feature r7rs) (not no-such-feature)) 'and-or-not) (else 'no))
    "and, or and not")
(test-equal '(registered on-disk absent)
    (list (cond-expand ((library (scheme base)) 'registered) (else 'no))
          (cond-expand ((library (tests libs never-imported)) 'on-disk) (else 'no))
          (cond-expand ((library (no such library)) 'present) (else 'absent)))
    "library requirements")
(cond-expand (s2 (define defined-by-cond-expand 'top)) (else))
(test-equal 'top defined-by-cond-expand "cond-expand at top level can define")
(test-equal #t (eq? (if #f #f) (cond-expand (no-such-feature 1))) "no clause holds")
(test-equal "cond-expand: else must be the last clause"
    (guard (e (#t (error-object-message e))) (eval '(cond-expand (else 1) (r7rs 2)) (interaction-environment)))
    "else must be last")

(define-library (test expanded)
  (export which)
  (cond-expand
    ((library (scheme base)) (import (scheme base)))
    (else (import (s2))))
  (cond-expand
    (no-such-feature (begin (define which 'wrong)))
    (s2 (begin (define which 'right)))))
(import (test expanded))
(test-equal 'right which "cond-expand declarations in define-library")
