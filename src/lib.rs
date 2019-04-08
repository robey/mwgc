//! A sesame-seed-sized heap with a tri-color, tracing, conservative,
//! incremental, _non_-compacting garbage collector, for implementing a tiny
//! language on tiny hardware.
//!
//! It's simple, _not_ thread safe, and efficient for allocations up to about
//! 512 bytes and heaps under about 1MB.
//!
//! Here's an example of creating a 256-byte heap on the stack, using it to
//! allocate two different objects, and then running the garbage collector to
//! reap one of them:
//!
//! ```rust
//! use mwgc::Heap;
//!
//! #[derive(Default)]
//! struct Toaster { a: u32 }
//!
//! let mut data: [u8; 256] = [0; 256];
//! let mut h = Heap::from_bytes(&mut data);
//! let o1 = h.allocate_object::<Toaster>().unwrap();
//! h.gc(&[ o1 ]);
//! ```

#![no_std]

#[macro_use]
extern crate static_assertions;

mod color_map;
mod free_list;
mod heap;
mod memory;
mod string_buffer;

pub use self::heap::{Heap, HeapStats};
pub use self::memory::Memory;
pub use self::string_buffer::StringBuffer;

/// how many bytes are in each block of memory?
/// smaller means more overhead wasted for tracking memory. larger means more wasted memory.
const BLOCK_SIZE_BYTES: usize = 16;

// block size must be big enough to hold linking info for the free list.
const_assert!(block_size; BLOCK_SIZE_BYTES >= free_list::FREE_BLOCK_SIZE);


// odd that this isn't in the stdlib, but apparently neither is divmod!
fn div_ceil(numerator: usize, denominator: usize) -> usize {
    let floor = numerator / denominator;
    let rem = numerator % denominator;
    if rem == 0 { floor } else { floor + 1 }
}

fn floor_to(n: usize, chunk: usize) -> usize {
    n / chunk * chunk
}

fn ceil_to(n: usize, chunk: usize) -> usize {
    div_ceil(n, chunk) * chunk
}
