(use-modules (guix packages)
             (gnu packages llvm)
             (gnu packages maths)
             (gnu packages rust))
             

(packages->manifest (list rust-1.65 `(,rust-1.65 "cargo") clang-toolchain hdf5))