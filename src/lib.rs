use core::fmt;
use core::mem::size_of;
use core::slice;

#[macro_use]
extern crate static_assertions;

pub mod block_colors;
pub mod free_list;

pub use self::block_colors::{BlockRange, BLOCKS_PER_COLORMAP_BYTE, Color, ColorMap};
pub use self::free_list::{FreeBlock, FreeBlockPtr, FreeList, FreeListIterator, FREE_BLOCK_SIZE};

/// configurable things:
/// how many bytes are in each block of memory?
const BLOCK_SIZE_BYTES: usize = 16;

// const WORD_SIZE_BYTES: usize = size_of::<usize>();
// const WORD_SIZE_BITS: usize = WORD_SIZE_BYTES * 8;

// const BLOCK_SIZE_WORDS: usize = BLOCK_SIZE_BYTES / WORD_SIZE_BYTES;


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

fn ceil_to(n: usize, chunk: usize) -> usize {
    div_ceil(n, chunk) * chunk
}


#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpanType {
    Color(Color),
    Free
}

#[derive(Clone, Copy, PartialEq)]
pub struct HeapSpan {
    pub start: *const u8,
    pub end: *const u8,
    pub span_type: SpanType,
}

impl HeapSpan {
    fn from_free_block(block: &'static FreeBlock) -> HeapSpan {
        HeapSpan { start: block.start(), end: block.end(), span_type: SpanType::Free }
    }

    fn from_block_range(heap: &Heap, range: BlockRange) -> HeapSpan {
        HeapSpan {
            start: heap.address_of(range.start),
            end: heap.address_of(range.end),
            span_type: SpanType::Color(range.color),
        }
    }
}

impl fmt::Debug for HeapSpan {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.span_type {
            SpanType::Free => write!(f, "FREE"),
            SpanType::Color(color) => write!(f, "{:?}", color),
        }?;
        write!(f, "[{}]", (self.end as usize) - (self.start as usize))
    }
}


pub struct Heap {
    pub start: *const u8,
    pub end: *const u8,
    color_map: ColorMap,
    blocks: usize,
    pub current_color: Color,

    free_list: FreeList,
}

impl Heap {
    pub fn new(memory: &'static mut [u8]) -> Heap {
        // total heap = pool + color_map, and pool is just color_map_size * blocks_per_colormap_byte * block_size
        // so color_map_size = heap size / (1 + bpm * bs)
        let divisor = 1 + BLOCKS_PER_COLORMAP_BYTE * BLOCK_SIZE_BYTES;
        let color_map_size = div_ceil(memory.len(), divisor);
        let pool_size = floor_to(memory.len() - color_map_size, BLOCK_SIZE_BYTES);
        let (pool_data, color_data) = memory.split_at_mut(memory.len() - color_map_size);
        let blocks = pool_size / BLOCK_SIZE_BYTES;

        // all of memory is free.
        let pool = unsafe { slice::from_raw_parts_mut(pool_data.as_mut_ptr(), pool_size) };
        Heap {
            start: pool_data.as_ptr(),
            end: unsafe { pool_data.as_ptr().offset(pool_size as isize) },
            color_map: ColorMap::new(color_data),
            blocks,
            current_color: Color::Blue,
            free_list: FreeList::new(pool)
        }
    }

    // for tests:
    pub fn from_raw<T>(memory: &mut T) -> Heap {
        Heap::new(unsafe { &mut *(slice::from_raw_parts_mut(memory as *mut T as *mut u8, size_of::<T>())) })
    }

    #[inline]
    fn address_of(&self, block: usize) -> *const u8 {
        ((self.start as usize) + block * BLOCK_SIZE_BYTES) as *const u8
    }

    #[inline]
    fn block_of(&self, p: *const u8) -> usize {
        ((p as usize) - (self.start as usize)) / BLOCK_SIZE_BYTES
    }

    #[inline]
    fn block_range_of(&self, memory: &'static [u8], color: Color) -> BlockRange {
        let start = self.block_of(memory.as_ptr());
        let end = start + memory.len() / BLOCK_SIZE_BYTES;
        BlockRange { start, end, color }
    }

    fn get_range(&self, p: *const u8, max: *const u8) -> BlockRange {
        self.color_map.get_range(self.block_of(p), self.block_of(max))
    }

    pub fn allocate(&mut self, amount: usize) -> Option<&'static [u8]> {
        if let Some(memory) = self.free_list.allocate(ceil_to(amount, BLOCK_SIZE_BYTES)) {
            self.color_map.set_range(self.block_range_of(memory, self.current_color));
            Some(memory)
        } else {
            None
        }
    }

    pub fn iter(&self) -> HeapIterator {
        HeapIterator { heap: self, next_free: self.free_list.first(), current: self.start }
    }

    pub fn dump(&self) -> String {
        self.iter().map(|span| { format!("{:?}", span) }).collect::<Vec<String>>().join(", ")
    }
}

impl fmt::Debug for Heap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Heap(pool={:?}, blocks={}x{}, ", self.start, self.blocks, BLOCK_SIZE_BYTES)?;
        if f.alternate() {
            write!(f, "{}", self.dump())?;
        } else {
            write!(f, "{:?}, {:?}", self.color_map, self.free_list)?;
        }
        write!(f, ")")
    }
}


pub struct HeapIterator<'a> {
    heap: &'a Heap,
    next_free: Option<FreeBlockPtr>,
    current: *const u8,
}

impl<'a> Iterator for HeapIterator<'a> {
    type Item = HeapSpan;

    // tricky: there are two lists to traverse in tandem. if the current
    // pointer is the next in the free list, that takes precedence.
    // otherwise, use the block map, but don't let it follow a chain past
    // the next free span.
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.heap.end { return None }

        if let Some(free) = self.next_free {
            if free.block().start() == self.current {
                self.next_free = free.next();
                self.current = free.block().end();
                return Some(HeapSpan::from_free_block(free.block()));
            }
        }

        let limit = self.next_free.map(|p| p.block() as *const FreeBlock as *const u8).unwrap_or(self.heap.end);
        let span = self.heap.get_range(self.current, limit);
        self.current = self.heap.address_of(span.end);
        Some(HeapSpan::from_block_range(self.heap, span))
    }
}


#[cfg(test)]
mod tests {
    use crate::Heap;

    #[test]
    fn new_heap() {
        let mut data: [u8; 256] = [0; 256];
        let h = Heap::from_raw(&mut data);
        assert_eq!(h.start, &data[0] as *const u8);
        assert_eq!(h.end, unsafe { h.start.offset(240) });
        assert_eq!(h.dump(), "FREE[240]");
    }

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::from_raw(&mut data);
        let alloc = h.allocate(32);
        assert!(alloc.is_some());
        if let Some(memory) = alloc {
            assert_eq!(memory.len(), 32);
            assert_eq!(h.dump(), "Blue[32], FREE[208]");
        }
    }
}
