use core::fmt;
use core::fmt::{Debug, Display};
use core::mem::size_of;

#[macro_use]
extern crate static_assertions;

/// configurable things:
/// how many bytes are in each block of memory?
const BLOCK_SIZE_BYTES: usize = 16;

const WORD_SIZE_BYTES: usize = size_of::<usize>();
const WORD_SIZE_BITS: usize = WORD_SIZE_BYTES * 8;

const BLOCK_SIZE_WORDS: usize = BLOCK_SIZE_BYTES / WORD_SIZE_BYTES;

// we need to reserve 2 bits per block for tracking.
const BLOCKS_PER_METADATA_BYTE: usize = 8 / 2;

// each free block is part of a linked list.
pub struct FreeBlock {
    next: *mut FreeBlock,
    size: usize,
}

// block size must be big enough to hold linking info for the free list.
const_assert!(block_size; BLOCK_SIZE_BYTES >= size_of::<FreeBlock>());


// odd that this isn't in the stdlib, but apparently neither is divmod!
fn div_ceil(numerator: usize, denominator: usize) -> usize {
    let floor = numerator / denominator;
    let rem = numerator % denominator;
    if rem == 0 { floor } else { floor + 1 }
}

fn floor_to(n: usize, chunk: usize) -> usize {
    n / chunk * chunk
}


#[derive(Debug)]
pub struct Heap<'a> {
    pool: &'a mut [u8],
    metadata: &'a mut [u8],
    blocks: usize,

    free: Option<*mut FreeBlock>,
}

impl<'a> Heap<'a> {
    // pub fn from_data<T>(data: &mut T) -> Heap {
    //     Heap::new(data as *mut T as *mut usize, size_of::<T>() / WORD_SIZE_BYTES)
    // }

    pub fn new(memory: &'a mut [u8]) -> Heap<'a> {
        // total heap = pool + metadata, and pool is just metadata * blocks_per_metadata * block_size
        // so metadata size = heap size / (1 + bpm * bs)
        let divisor = 1 + BLOCKS_PER_METADATA_BYTE * BLOCK_SIZE_BYTES;
        let metadata_size = div_ceil(memory.len(), divisor);
        let pool_size = floor_to(memory.len() - metadata_size, BLOCK_SIZE_BYTES);
        let (pool, metadata) = memory.split_at_mut(memory.len() - metadata_size);
        let blocks = pool_size / BLOCK_SIZE_BYTES;

        println!("metadata size={}", metadata_size);

        Heap { pool, metadata, blocks, free: None }
    }
}


impl Display for Heap<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Heap(pool={}, metadata={:?})", self.blocks, self.metadata.len())
    }
}

#[cfg(test)]
mod tests {
    use crate::Heap;

    #[test]
    fn make() {
        let mut data: [u8; 256] = [0; 256];
        println!("{}", Heap::new(&mut data));
    }

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
