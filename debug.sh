#!/bin/sh

set -eax

rm -rf target
cargo rustc --release -- --emit=llvm-ir,asm,obj
