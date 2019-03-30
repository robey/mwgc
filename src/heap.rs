use core::{fmt, mem, ptr, slice};

use crate::{BLOCK_SIZE_BYTES, ceil_to, div_ceil, floor_to};
use crate::color_map::{BlockRange, BLOCKS_PER_COLORMAP_BYTE, Color, ColorMap};
use crate::free_list::{FreeBlock, FreeList, FreeListSpan};
use crate::memory::Memory;

#[derive(Clone, Copy, PartialEq)]
enum SpanType {
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
struct HeapSpan<'a> {
    pub start: *mut u8,
    pub end: *mut u8,
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


struct HeapIterator<'a> {
    heap: &'a Heap<'a>,
    free_list_span: FreeListSpan<'a>,
    current: *mut u8,
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


/// Stats returned from [`Heap::get_stats`](struct.Heap.html#method.get_stats).
pub struct HeapStats {
    /// total bytes available in the heap: provided memory minus overhead
    pub total_bytes: usize,

    /// bytes free for future allocations right now
    pub free_bytes: usize,

    /// for testing & debugging: the extent of the pool
    pub start: *const u8,

    /// for testing & debugging: the extent of the pool
    pub end: *const u8,
}


#[derive(PartialEq)]
enum Phase {
    QUIET, MARKING, MARKED
}

/// Takes ownership of a block of [`Memory`](struct.Memory.html), hands out
/// chunks of it, and garbage collects unused chunks on demand.
///
/// The heap is organized into allocatable blocks, with a small region
/// reserved as a bitmap. The block size is configured by a compile-time
/// constant, using a default of 16 bytes. Each block has a 2-bit "color"
/// in the bitmap, at a cost of about 2% overhead. One "color" is used to
/// mark continuations of contiguous spans, so each allocation can be
/// identified by a color followed by one or more "continue" markers. The
/// other 3 colors correspond to the white, gray, and black colors of
/// tri-color marking, although I've chosen to call them blue, green, and
/// "check".
///
/// Separately, a sorted free list is maintained by storing a "next" link
/// and size in each free block. This consumes 8 bytes on a 32-bit system,
/// limiting the minimum block size.
///
/// The heap object (its state) should consume about 9 words, or 36 bytes
/// on a 32-bit system.
pub struct Heap<'heap> {
    start: *mut u8,
    end: *mut u8,
    blocks: usize,
    color_map: ColorMap<'heap>,
    free_list: FreeList<'heap>,

    // gc state:
    current_color: Color,
    phase: Phase,

    // for marking:
    check_start: *const u8,
    check_end: *const u8,
}

impl<'heap> Heap<'heap> {
    /// Create a new heap out of a mutable chunk of memory.
    pub fn new(m: Memory<'heap>) -> Heap<'heap> {
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
            phase: Phase::QUIET,
            check_start: ptr::null(),
            check_end: ptr::null(),
        }
    }

    /// Create a new heap out of a mutable byte-slice.
    pub fn from_bytes(bytes: &'heap mut [u8]) -> Heap<'heap> {
        Heap::new(Memory::new(bytes))
    }

    #[inline]
    fn address_of(&self, block: usize) -> *mut u8 {
        ((self.start as usize) + block * BLOCK_SIZE_BYTES) as *mut u8
    }

    #[inline]
    fn block_of(&self, p: *const u8) -> usize {
        let mut b = ((p as usize) - (self.start as usize)) / BLOCK_SIZE_BYTES;
        while self.color_map.get(b) == Color::Continue { b -= 1 }
        b
    }

    #[inline]
    fn block_range_of(&self, memory: &Memory, color: Color) -> BlockRange {
        let start = self.block_of(memory.start());
        let end = start + memory.len() / BLOCK_SIZE_BYTES;
        BlockRange { start, end, color }
    }

    fn is_block(&self, p: *const u8) -> bool {
        p >= self.start && p < self.end && (p as usize) % mem::size_of::<usize>() == 0
    }

    fn get_range(&self, p: *const u8) -> BlockRange {
        self.color_map.get_range(self.block_of(p))
    }

    /// Request a `amount` bytes of memory. The size will be rounded up to
    /// a multiple of the block size. Returns `None` if a block of memory
    /// that big isn't available,
    pub fn allocate(&mut self, amount: usize) -> Option<Memory<'heap>> {
        if let Some(mut m) = self.free_list.allocate(ceil_to(amount, BLOCK_SIZE_BYTES)) {
            let color = if self.phase == Phase::MARKING { Color::Check } else { self.current_color };
            self.color_map.set_range(self.block_range_of(&m, color));
            if self.phase == Phase::MARKING {
                self.add_to_check_span(m.start());
            }
            m.clear();
            Some(m)
        } else {
            None
        }
    }

    /// Request enough memory to hold an object of type `T`. The object will
    /// be initialized to its default value. Returns `None` if a block of
    /// memory that big isn't available.
    pub fn allocate_object<T: Default>(&mut self) -> Option<&'heap mut T> {
        self.allocate(mem::size_of::<T>()).map(|m| {
            let obj: &'heap mut T = unsafe { mem::transmute(m.inner().as_mut_ptr()) };
            *obj = T::default();
            obj
        })
    }

    /// Request enough memory to hold an object of type `T` followed by
    /// dynamic-sized padding. The object will be initialized to its default
    /// value. Returns `None` if a block of memory that big isn't available.
    pub fn allocate_dynamic_object<T: Default>(&mut self, padding: usize) -> Option<&'heap mut T> {
        self.allocate(mem::size_of::<T>() + padding).map(|m| {
            let obj: &'heap mut T = unsafe { mem::transmute(m.inner().as_mut_ptr()) };
            *obj = T::default();
            obj
        })
    }

    /// Request enough memory to hold an array of `count` objects of type `T`.
    /// Each object in the array will be initialized to its default value.
    /// Returns `None` if a block of memory that big isn't available.
    pub fn allocate_array<T: Default>(&mut self, count: usize) -> Option<&'heap mut [T]> {
        self.allocate(mem::size_of::<T>() * count).map(|m| unsafe {
            let array: &'heap mut [T] = slice::from_raw_parts_mut(mem::transmute(m.inner().as_mut_ptr()), count);
            for item in array.iter_mut() {
                *item = T::default();
            }
            array
        })
    }

    /// Given an object that was allocated on this heap, how many bytes were
    /// allocated to it?
    pub fn size_of<T>(&self, obj: &T) -> usize {
        let range = self.get_range(obj as *const T as *const u8);
        (self.address_of(range.end) as usize) - (self.address_of(range.start) as usize)
    }

    /// Give back an allocation without waiting for a GC round.
    pub fn retire(&mut self, m: Memory<'heap>) {
        self.color_map.free_range(self.block_range_of(&m, Color::Check));
        self.free_list.retire(m);
    }

    /// Give back an allocated object without waiting for a GC round.
    pub fn retire_object<T>(&mut self, obj: &'heap mut T) {
        let range = self.get_range(obj as *mut T as *const T as *const u8);
        let m = Memory::from_addresses(self.address_of(range.start), self.address_of(range.end));
        self.color_map.free_range(range);
        self.free_list.retire(m);
    }

    /// Start the first phase of garbage collection. This is only useful if
    /// you want tight control over latency -- otherwise, you should call
    /// [`gc()`](struct.Heap.html#method.gc).
    ///
    /// `roots` must be a slice of references to objects in the heap which
    /// are the roots of your object graph. Successive calls to
    /// [`mark_round()`](struct.Heap.html#method.mark_round) will follow the
    /// object graph and mark all live objects. A follow-on call to
    /// [`sweep()`](struct.Heap.html#method.sweep) will free the
    /// remaining spans.
    ///
    /// **Important**: If you modify any objects before `mark_round()` returns
    /// `true`, you must notify the garbage collector to (re)check the object
    /// you modified by calling
    /// [`mark_check`](struct.Heap.html#method.mark_check).
    pub fn mark_start<T>(&mut self, roots: &[&T]) {
        assert!(self.phase == Phase::QUIET);
        self.check_start = ptr::null();
        self.check_end = ptr::null();
        self.current_color = self.current_color.opposite();
        for r in roots { self.check(*r as *const T as *const u8) }
        self.phase = Phase::MARKING;
    }

    /// Do one "round" of the mark phase of garbage collection. This is only
    /// useful if you want tight control over latency -- otherwise, you
    /// should call [`gc()`](struct.Heap.html#method.gc).
    ///
    /// Walk the current mark span, following links from marked objects to
    /// find objects to check on the next round.
    ///
    /// Returns true if this phase is over, and all live objects have been
    /// marked. You must call [`sweep()`](struct.Heap.html#method.sweep) to
    /// finish the collection and free up memory.
    ///
    /// **Important**: If you modify any objects before this function returns
    /// `true`, you must notify the garbage collector to (re)check the object
    /// you modified by calling
    /// [`mark_check`](struct.Heap.html#method.mark_check).
    pub fn mark_round(&mut self) -> bool {
        assert!(self.phase == Phase::MARKING);
        if self.check_start == ptr::null() {
            self.phase = Phase::MARKED;
            return true;
        }

        let (start, end) = (self.check_start, self.check_end);
        self.check_start = ptr::null();
        self.check_end = ptr::null();

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
                    p = ((p as usize) + mem::size_of::<usize>()) as *const usize;
                }
                self.color_map.set(self.block_of(current), self.current_color);
            }
            current = end_addr as *const u8;
        }

        // we're done marking if there's no new span to check.
        if self.check_start == ptr::null() {
            self.phase = Phase::MARKED;
            true
        } else {
            false
        }
    }

    /// Do the mark phase of garbage collection (the first of two phases).
    ///
    /// `roots` must be a slice of references to objects in the heap which
    /// are the roots of your object graph. Any object that isn't directly
    /// or indirectly referenced by these roots will be freed by a following
    /// call to [`sweep()`](struct.Heap.html#method.sweep).
    ///
    /// This is the equivalent of `heap.mark_start(roots); while !heap.mark_round() {};`.
    pub fn mark<T>(&mut self, roots: &[&T]) {
        self.mark_start(roots);
        while !self.mark_round() {}
    }

    /// Mark an object to be re-checked because it's been modified during the
    /// mark phase of garbage collection.
    ///
    /// This function is only useful if you use the incremental GC calls
    /// `mark_start` and `mark_round`.
    pub fn mark_check<T>(&mut self, obj: &T) {
        let p = obj as *const T as *const u8;
        if self.is_block(p) {
            let block = self.block_of(p);
            self.color_map.set(block, Color::Check);
            self.add_to_check_span(p);
        }
    }

    fn check(&mut self, p: *const u8) {
        if self.is_block(p) {
            let block = self.block_of(p);
            if self.color_map.get(block) == self.current_color.opposite() {
                self.color_map.set(block, Color::Check);
                self.add_to_check_span(p);
            }
        }
    }

    fn add_to_check_span(&mut self, p: *const u8) {
        if self.check_start == ptr::null() || self.check_start > p {
            self.check_start = p;
        }
        if self.check_end == ptr::null() || self.check_end < p {
            self.check_end = p;
        }
    }

    /// For debugging and tests, report the current range of addresses that
    /// will be scanned on the next `mark_round`.
    pub fn get_mark_range(&self) -> (*const u8, *const u8) {
        (self.check_start, self.check_end)
    }

    /// Sweep through the heap and move every un-marked span of memory into
    /// the free list. This is the 2nd and final phase of garbage collection.
    pub fn sweep(&mut self) {
        assert!(self.phase == Phase::MARKED);
        self.iter().filter(|span| span.span_type == SpanType::Color(self.current_color.opposite())).for_each(|span| {
            let m = Memory::from_addresses(span.start, span.end);
            span.free_list_span.insert(m);
        });
        self.phase = Phase::QUIET;
    }

    /// Do an entire GC round, freeing any currently unused memory.
    ///
    /// `roots` must be a slice of references to objects in the heap which
    /// are the roots of your object graph. Any object that isn't directly
    /// or indirectly referenced by these roots will be freed.
    ///
    /// This is the equivalent of `heap.mark(roots); heap.sweep();`.
    pub fn gc<T>(&mut self, roots: &[&T]) {
        self.mark(roots);
        self.sweep();
    }

    fn iter(&self) -> HeapIterator {
        HeapIterator::new(self)
    }

    /// For debugging: generate a string listing the size and color of each
    /// span of memory.
    pub fn dump(&self) -> String {
        self.iter().map(|span| { format!("{:?}", span) }).collect::<Vec<String>>().join(", ")
    }

    /// For debugging: generate a string listing _only_ the color of each
    /// span of memory.
    pub fn dump_spans(&self) -> String {
        self.iter().map(|span| { format!("{:?}", span.span_type) }).collect::<Vec<String>>().join(", ")
    }

    /// Return an object listing the free & total bytes of this heap.
    pub fn get_stats(&self) -> HeapStats {
        HeapStats {
            total_bytes: self.blocks * BLOCK_SIZE_BYTES,
            free_bytes: self.free_list.bytes(),
            start: self.start,
            end: self.end,
        }
    }
}

impl<'a> fmt::Debug for Heap<'a> {
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
