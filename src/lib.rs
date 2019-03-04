use core::fmt;
use core::mem::{size_of, transmute};
use core::ptr;

#[macro_use]
extern crate static_assertions;

pub mod color_map;
pub mod free_list;
pub mod memory;

pub use self::color_map::{BlockRange, BLOCKS_PER_COLORMAP_BYTE, Color, ColorMap};
pub use self::free_list::{FreeBlock, FreeBlockPtr, FreeList, FreeListIterator, FreeListSpan, FREE_BLOCK_SIZE};
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


#[derive(Clone, Copy)]
pub struct HeapSpan<'a> {
    pub start: *const u8,
    pub end: *const u8,
    pub span_type: SpanType,
    pub free_list_span: FreeListSpan<'a>,
}

impl<'a> HeapSpan<'a> {
    fn from_free_block(block: &'a FreeBlock, free_list_span: FreeListSpan<'a>) -> HeapSpan<'a> {
        HeapSpan { start: block.start(), end: block.end(), span_type: SpanType::Free, free_list_span }
    }

    fn from_block_range(heap: &Heap, range: BlockRange, free_list_span: FreeListSpan<'a>) -> HeapSpan<'a> {
        HeapSpan {
            start: heap.address_of(range.start),
            end: heap.address_of(range.end),
            span_type: SpanType::Color(range.color),
            free_list_span,
        }
    }
}

impl<'a> fmt::Debug for HeapSpan<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if f.alternate() {
            write!(f, "{:?}[{:?} - {:?}, between {:?} - {:?}]", self.span_type, self.start, self.end, self.free_list_span.insert_point, self.free_list_span.ptr)
        } else {
            write!(f, "{:?}[{}]", self.span_type, (self.end as usize) - (self.start as usize))
        }
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

    // give back an allocation without waiting for a GC round.
    pub fn retire_object<T>(&mut self, obj: &mut T) {
        let range = self.get_range(obj as *mut T as *const T as *const u8);
        let m = Memory::from_addresses(self.address_of(range.start), self.address_of(range.end));
        self.color_map.free_range(range);
        self.free_list.retire(m);
    }

    // set up a mark phase, starting from these roots.
    pub fn mark_start(&mut self, roots: &[*const u8]) {
        self.check_start = core::ptr::null();
        self.check_end = core::ptr::null();
        self.current_color = self.current_color.opposite();
        for r in roots { self.check(*r) }
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
                    self.check(word);
                    p = ((p as usize) + size_of::<usize>()) as *const usize;
                }
                self.color_map.set(self.block_of(current), self.current_color);
            }
            current = end_addr as *const u8;
        }

        // we're done marking if there's no new span to check.
        self.check_start == core::ptr::null()
    }

    pub fn mark(&mut self, roots: &[*const u8]) {
        self.mark_start(roots);
        while !self.mark_round() {}
    }

    fn check(&mut self, p: *const u8) {
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

    pub fn get_mark_range(&self) -> (*const u8, *const u8) {
        (self.check_start, self.check_end)
    }

    // free any spans that weren't marked
    pub fn sweep(&mut self) {
        self.iter().filter(|span| span.span_type == SpanType::Color(self.current_color.opposite())).for_each(|span| {
            let m = Memory::from_addresses(span.start, span.end);
            span.free_list_span.insert(m);
        });
    }

    fn iter(&self) -> HeapIterator {
        HeapIterator::new(self)
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
    free_list_span: FreeListSpan<'a>,
    current: *const u8,
}

impl<'a> HeapIterator<'a> {
    fn new(heap: &'a Heap) -> HeapIterator<'a> {
        // there is always at least one span, because the final null pointer
        // is yielded. we stop calling `next()` at that point, so we never
        // get to the `None` end of the iteration.
        let free_list_span = heap.free_list.iter_span().next().unwrap();
        HeapIterator { heap, free_list_span, current: heap.start }
    }
}

impl<'a> Iterator for HeapIterator<'a> {
    type Item = HeapSpan<'a>;

    // tricky: there are two lists to traverse in tandem. if the current
    // pointer is the next in the free list, that takes precedence.
    // otherwise, use the block map.
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.heap.end { return None }

        if let Some(free) = self.free_list_span.ptr.ptr {
            // did they insert a new free item behind us when we gave out the last span?
            if free.start() < self.current {
                self.free_list_span = self.free_list_span.next().unwrap();
                return self.next();
            }

            if free.start() == self.current {
                let free_span: FreeListSpan<'a> = self.free_list_span;
                // there is always at least one span, because the final null pointer
                // is yielded. we stop calling `next()` at that point, so we never
                // get to the `None` end of the iteration.
                self.free_list_span = free_span.next().unwrap();
                self.current = free.end();
                return Some(HeapSpan::from_free_block(free, free_span));
            }
        }

        let span = self.heap.get_range(self.current);
        self.current = self.heap.address_of(span.end);
        Some(HeapSpan::from_block_range(self.heap, span, self.free_list_span))
    }
}
