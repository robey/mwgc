use core::fmt;
use core::mem::{size_of, transmute};
use core::ptr;

#[macro_use]
extern crate static_assertions;

pub mod block_colors;
pub mod free_list;
pub mod memory;

pub use self::block_colors::{BlockRange, BLOCKS_PER_COLORMAP_BYTE, Color, ColorMap};
pub use self::free_list::{FreeBlock, FreeBlockPtr, FreeList, FreeListIterator, FREE_BLOCK_SIZE};
pub use self::memory::{Memory};


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


#[derive(Clone, Copy, PartialEq)]
pub enum SpanType {
    Color(Color),
    Free
}

impl fmt::Debug for SpanType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SpanType::Free => write!(f, "FREE"),
            SpanType::Color(color) => write!(f, "{:?}", color),
        }
    }
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
        write!(f, "{:?}[{}]", self.span_type, (self.end as usize) - (self.start as usize))
    }
}


// 9 words
pub struct Heap {
    pub start: *const u8,
    pub end: *const u8,
    blocks: usize,
    color_map: ColorMap,
    free_list: FreeList,
    pub current_color: Color,

    // for marking:
    check_start: *const u8,
    check_end: *const u8,
}

impl Heap {
    pub fn new(m: Memory) -> Heap {
        // total heap = pool + color_map, and pool is just color_map_size * blocks_per_colormap_byte * block_size
        // so color_map_size = heap size / (1 + bpm * bs)
        let divisor = 1 + BLOCKS_PER_COLORMAP_BYTE * BLOCK_SIZE_BYTES;
        let color_map_size = div_ceil(m.len(), divisor);
        let pool_size = floor_to(m.len() - color_map_size, BLOCK_SIZE_BYTES);
        let len = m.len();
        let (pool_data, color_data) = m.split_at(len - color_map_size);
        let blocks = pool_size / BLOCK_SIZE_BYTES;

        // all of memory is free.
        let pool = pool_data.split_at(pool_size).0;
        Heap {
            start: pool.start(),
            end: pool.end(),
            blocks,
            color_map: ColorMap::new(color_data),
            free_list: FreeList::new(pool),
            current_color: Color::Blue,
            check_start: ptr::null(),
            check_end: ptr::null(),
        }
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
    fn block_range_of(&self, memory: &Memory, color: Color) -> BlockRange {
        let start = self.block_of(memory.start());
        let end = start + memory.len() / BLOCK_SIZE_BYTES;
        BlockRange { start, end, color }
    }

    fn is_block(&self, p: *const u8) -> bool {
        p >= self.start && p < self.end && ((p as usize) - (self.start as usize)) % BLOCK_SIZE_BYTES == 0
    }

    fn get_range(&self, p: *const u8) -> BlockRange {
        self.color_map.get_range(self.block_of(p))
    }

    pub fn allocate(&mut self, amount: usize) -> Option<Memory> {
        if let Some(mut m) = self.free_list.allocate(ceil_to(amount, BLOCK_SIZE_BYTES)) {
            self.color_map.set_range(self.block_range_of(&m, self.current_color));
            m.clear();
            Some(m)
        } else {
            None
        }
    }

    pub fn allocate_object<T>(&mut self) -> Option<&'static mut T> {
        self.allocate(size_of::<T>()).map(|m| unsafe { transmute(m.inner().as_mut_ptr()) } )
    }

    // give back an allocation without waiting for a GC round.
    pub fn retire(&mut self, m: Memory) {
        self.color_map.free_range(self.block_range_of(&m, Color::Check));
        self.free_list.retire(m);
    }

    // set up a mark phase, starting from these roots.
    pub fn mark_start(&mut self, roots: &[*const u8]) {
        self.check_start = core::ptr::null();
        self.check_end = core::ptr::null();
        self.current_color = self.current_color.opposite();
        for r in roots { self.mark(*r) }
    }

    // do one "round" of marking. if we're done after this round, returns true.
    pub fn mark_round(&mut self) -> bool {
        if self.check_start == core::ptr::null() { return true }

        let (start, end) = (self.check_start, self.check_end);
        self.check_start = core::ptr::null();
        self.check_end = core::ptr::null();

        let mut current = start;
        while current <= end {
            let r = self.get_range(current);
            let start_addr = self.address_of(r.start) as *const usize;
            let end_addr = self.address_of(r.end) as *const usize;
            if r.color == Color::Check {
                // pretend the whole memory block is words, and traverse it, marking anything we find.
                let mut p = start_addr;
                while p < end_addr {
                    let word = unsafe { *p } as *const u8;
                    self.mark(word);
                    p = ((p as usize) + size_of::<usize>()) as *const usize;
                }
                self.color_map.set(self.block_of(current), self.current_color);
            }
            current = end_addr as *const u8;
        }

        // we're done marking if there's no new span to check.
        self.check_start == core::ptr::null()
    }

    fn mark(&mut self, p: *const u8) {
        println!("try {:?} in {:?}", p, self);
        if self.is_block(p) {
            let block = self.block_of(p);
            if self.color_map.get(block) == self.current_color.opposite() {
                self.color_map.set(block, Color::Check);
                if self.check_start == core::ptr::null() || self.check_start > p {
                    self.check_start = p;
                }
                if self.check_end == core::ptr::null() || self.check_end < p {
                    self.check_end = p;
                }
            }
        }
    }

    fn iter(&self) -> HeapIterator {
        HeapIterator { heap: self, next_free: self.free_list.first(), current: self.start }
    }

    pub fn dump(&self) -> String {
        self.iter().map(|span| { format!("{:?}", span) }).collect::<Vec<String>>().join(", ")
    }

    pub fn dump_spans(&self) -> String {
        self.iter().map(|span| { format!("{:?}", span.span_type) }).collect::<Vec<String>>().join(", ")
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
    next_free: FreeBlockPtr,
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

        if let Some(free) = self.next_free.ptr {
            if free.start() == self.current {
                self.next_free = free.next;
                self.current = free.end();
                return Some(HeapSpan::from_free_block(free));
            }
        }

        let span = self.heap.get_range(self.current);
        self.current = self.heap.address_of(span.end);
        Some(HeapSpan::from_block_range(self.heap, span))
    }
}


#[cfg(test)]
mod tests {
    use core::mem::size_of;
    use crate::{Heap, Memory};

    // used to test the GC
    struct Sample {
        p: *const Sample,
        number: usize,
        next: *const Sample,
        prev: *const Sample,
    }

    impl Sample {
        pub fn ptr(&self) -> *const u8 {
            self as *const Sample as *const u8
        }
    }


    #[test]
    fn new_heap() {
        let mut data: [u8; 256] = [0; 256];
        let h = Heap::new(Memory::take(&mut data));
        assert_eq!(h.start, &data[0] as *const u8);
        assert_eq!(h.end, unsafe { h.start.offset(240) });
        assert_eq!(h.dump(), "FREE[240]");
    }

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::new(Memory::take(&mut data));
        let alloc = h.allocate(32);
        assert!(alloc.is_some());
        if let Some(m) = alloc {
            assert_eq!(m.len(), 32);
            assert_eq!(h.dump(), "Blue[32], FREE[208]");
        }
    }

    #[test]
    fn retire() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::new(Memory::take(&mut data));
        let a1 = h.allocate(32);
        let a2 = h.allocate(32);
        assert!(a1.is_some() && a2.is_some());
        if let Some(m) = a1 {
            h.retire(m);
            assert_eq!(h.dump(), "FREE[32], Blue[32], FREE[176]");
            assert_eq!(format!("{:?}", h.color_map), "ColorMap(CCB.CCCCCCCCCCCC)");
        }
    }

    #[test]
    fn mark_simple() {
        let mut data: [u8; 256] = [0; 256];
        let mut h = Heap::new(Memory::take(&mut data));
        let o1 = h.allocate_object::<Sample>().unwrap();
        let o2 = h.allocate_object::<Sample>().unwrap();
        let o3 = h.allocate_object::<Sample>().unwrap();
        let o4 = h.allocate_object::<Sample>().unwrap();
        assert_eq!(h.dump_spans(), "Blue, Blue, Blue, Blue, FREE");

        // leave o3 stranded. make o1 point to o2, which points to o4 and back to o1.
        o1.p = o2 as *const Sample;
        o2.p = 455 as *const Sample;
        o2.next = o4 as *const Sample;
        o2.prev = o1 as *const Sample;

        h.mark_start(&[ o1.ptr() ]);
        assert_eq!(h.check_start, o1.ptr());
        assert_eq!(h.check_end, o1.ptr());
        assert_eq!(h.dump_spans(), "Check, Blue, Blue, Blue, FREE");

        assert!(!h.mark_round());
        assert_eq!(h.check_start, o2.ptr());
        assert_eq!(h.check_end, o2.ptr());
        assert_eq!(h.dump_spans(), "Green, Check, Blue, Blue, FREE");

        assert!(!h.mark_round());
        assert_eq!(h.check_start, o4.ptr());
        assert_eq!(h.check_end, o4.ptr());
        assert_eq!(h.dump_spans(), "Green, Green, Blue, Check, FREE");

        assert!(h.mark_round());
        assert_eq!(h.check_start, core::ptr::null());
        assert_eq!(h.check_end, core::ptr::null());
        assert_eq!(h.dump_spans(), "Green, Green, Blue, Green, FREE");
    }
}
