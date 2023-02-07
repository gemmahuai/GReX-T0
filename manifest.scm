(use-modules (guix packages)
             (gnu packages llvm)
             (gnu packages maths)
             (gnu packages rust))
             
(packages->manifest
    (list rust
          `(,rust "cargo")
          clang-toolchain
          hdf5))