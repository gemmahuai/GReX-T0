(use-modules (guix packages)
             (gnu packages llvm)
             (gnu packages maths)
             (gnu packages rust))

;; Grab the internal rust 1.65 define, until they export it
(define rust (@@ (gnu packages rust) rust-1.65))
             
(packages->manifest
    (list rust
          `(,rust "cargo")
          clang-toolchain
          hdf5))