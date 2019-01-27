use core::fmt;
use core::mem::size_of;
use core::slice;

#[macro_use]
extern crate static_assertions;

pub mod free_list;
use self::free_list::{FreeBlock, FreeBlockPtr, FreeList, FREE_BLOCK_SIZE};

/// configurable things:
/// how many bytes are in each block of memory?
const BLOCK_SIZE_BYTES: usize = 16;

const WORD_SIZE_BYTES: usize = size_of::<usize>();
const WORD_SIZE_BITS: usize = WORD_SIZE_BYTES * 8;

const BLOCK_SIZE_WORDS: usize = BLOCK_SIZE_BYTES / WORD_SIZE_BYTES;

// we need to reserve 2 bits per block for tracking.
const BLOCKS_PER_METADATA_BYTE: usize = 8 / 2;


// block size must be big enough to hold linking info for the free list.
const_assert!(block_size; BLOCK_SIZE_BYTES >= FREE_BLOCK_SIZE);


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
pub struct Heap {
    pool: &'static [u8],
    metadata: &'static [u8],
    blocks: usize,

    pub free: FreeList,
}

impl Heap {
    pub fn new(memory: &'static mut [u8]) -> Heap {
        // total heap = pool + metadata, and pool is just metadata * blocks_per_metadata * block_size
        // so metadata size = heap size / (1 + bpm * bs)
        let divisor = 1 + BLOCKS_PER_METADATA_BYTE * BLOCK_SIZE_BYTES;
        let metadata_size = div_ceil(memory.len(), divisor);
        let pool_size = floor_to(memory.len() - metadata_size, BLOCK_SIZE_BYTES);
        let (pooldata, metadata) = memory.split_at(memory.len() - metadata_size);
        let blocks = pool_size / BLOCK_SIZE_BYTES;

        // all of memory is free.
        let pool = unsafe { slice::from_raw_parts(pooldata.as_ptr(), pool_size) };
        Heap { pool, metadata, blocks, free: FreeList::new(pool) }
    }
}


impl fmt::Display for Heap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
            "Heap(pool={:?}, blocks={}x{}, metadata={:?}, free=[{:?}])",
            self.pool as *const _, self.blocks, BLOCK_SIZE_BYTES, self.metadata.len(), self.free
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::Heap;

    static mut DATA256: [u8; 256] = [0; 256];

    #[test]
    fn make() {
        println!("at {:?}", unsafe { &DATA256 as *const _ });
        println!("{}", Heap::new(unsafe { &mut DATA256 }));

        let h = Heap::new(unsafe { &mut DATA256 });
        h.free.list.block_mut().split(32);
        println!("{}", h);
    }

    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
