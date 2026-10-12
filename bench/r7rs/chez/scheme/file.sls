;;; (scheme file) for Chez Scheme: Chez's, replacing files as R7RS does
;;; Part of bench/r7rs (plan/04 P12); not a complete R7RS.
(library (scheme file)
  (export
    call-with-input-file call-with-output-file delete-file file-exists?
    open-binary-input-file open-binary-output-file open-input-file open-output-file with-input-from-file with-output-to-file)
  (import (except (chezscheme)
      call-with-output-file open-output-file with-output-to-file)
    (prefix (only (chezscheme)
      call-with-output-file open-output-file with-output-to-file)
      chez:))

  ;; Chez's refuse to replace an existing file; R7RS's replace it.
  (define (open-output-file name) (chez:open-output-file name 'truncate))
  (define (call-with-output-file name proc)
    (let ((p (open-output-file name)))
      (call-with-values (lambda () (proc p)) (lambda vals (close-port p) (apply values vals)))))
  (define (with-output-to-file name thunk) (chez:with-output-to-file name thunk 'truncate))
  (define (open-binary-input-file name) (open-file-input-port name))
  (define (open-binary-output-file name) (open-file-output-port name (file-options no-fail)))
)
