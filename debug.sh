#!/bin/sh

set -eax

rm -rf target
cargo rustc --release -- --emit=llvm-ir
cargo rustc --release -- --emit=asm
