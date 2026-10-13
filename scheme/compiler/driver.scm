;;; The driver (plan/08, C6): compiling a form, eval, dis, and loading a
;;; library file. With S2_COMPILER=scheme, s2 binds eval, dis and
;;; %load-library to eval-form, dis-form and load-library, so every
;;; compile goes through here. Loading never has to wait: an import of a
;;; library whose file isn't loaded loads it there and then
;;; (find-library).

(define (compile form env) (codegen (expand form env)))

;; eval (R7RS 6.12). The compiled form is called in tail position, so a
;; tail call to eval is still one.
(define (eval-form . args)
  (if (not (= (length args) 2))
      (fail (string-append "eval: wrong number of arguments: " (number->string (length args)))))
  (if (not (environment? (cadr args)))
      (fail "eval: the second argument must be an environment"))
  ((%template->procedure (compile (car args) (cadr args)))))

;; dis: compile a form for the interaction environment and print its
;; code, without running it.
(define (dis-form datum)
  (%disassemble (compile datum (interaction-environment))))

;; Load a library file, for an import that needs one: marked loaded
;; first, so a library that imports itself can't loop. From its compiled
;; file if that is fresh, else compiled.
(define (load-library path)
  (%library-file-loading path)
  (let retry ()
    (let ((cached (%cached-library path)))
      (cond ((string? cached) (load-library cached) (retry))
            (cached (for-each (lambda (thunk) (thunk)) cached))
            (else (compile-library path))))))

;; Compile a library file's forms in the interaction environment, running
;; each before compiling the next: a later define-library may import an
;; earlier one. If every form compiles, write the compiled file.
(define (compile-library path)
  (let ((env (interaction-environment))
        (templates '())
        (done #f))
    (dynamic-wind
     %library-compile-begin
     (lambda ()
       (for-each (lambda (form)
                   (let ((t (compile form env)))
                     (set! templates (cons t templates))
                     ((%template->procedure t))))
                 (%read-library-file path))
       (set! done #t))
     (lambda () (%library-compile-end path (and done (reverse templates)))))))
