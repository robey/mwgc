use core::{fmt, mem, slice};
use crate::memory::Memory;

// each free block is part of a linked list.

// a FreeBlockPtr has "interior mutability"
#[derive(Clone, Copy)]
pub struct FreeBlockPtr<'heap> {
    pub ptr: Option<&'heap FreeBlock<'heap>>,
}

const LAST: FreeBlockPtr = FreeBlockPtr { ptr: None };

impl<'heap> FreeBlockPtr<'heap> {
    pub fn new(m: Memory<'heap>, next: FreeBlockPtr<'heap>) -> FreeBlockPtr<'heap> {
        let block = FreeBlock::from_memory(m, next);
        FreeBlockPtr { ptr: Some(block) }
    }

    // attempt to allocate memory out of this block.
    pub fn allocate(&self, amount: usize) -> Option<Memory<'heap>> {
        let s = self.as_mut();
        s.ptr.and_then(|block| {
            if amount > block.size {
                None
            } else if block.size - amount < FREE_BLOCK_SIZE {
                // if there isn't enough left in this block for a new block, just use it all.
                s.ptr = block.next.ptr;
                Some(block.as_memory())
            } else {
                // split off a new alloc
                let (a1, a2) = block.as_memory().split_at(amount);
                s.ptr = Some(FreeBlock::from_memory(a2, block.next));
                Some(a1)
            }
        })
    }

    // the inserts will consume the memory if it was successfully inserted,
    // or return it if this isn't the right place.

    pub fn try_insert_before(&self, m: Memory<'heap>) -> Option<Memory<'heap>> {
        let s = self.as_mut();
        if let Some(block) = s.ptr.as_mut() {
            if block.start() > m.start() {
                // insert before the current block.
                let new_block = FreeBlock::from_memory(m, *self);
                new_block.check_merge_next();
                s.ptr = Some(new_block);
                return None
            }
        }
        Some(m)
    }

    pub fn try_insert_after(&self, m: Memory<'heap>) -> Option<Memory<'heap>> {
        let s = self.as_mut();
        match s.ptr.as_mut() {
            None => {
                // if this is the end, append.
                s.ptr = Some(FreeBlock::from_memory(m, LAST));
                None
            },
            Some(block) => {
                if block.end() == m.start() {
                    // merge to the end of this block.
                    block.as_mut().size += m.len();
                    block.as_mut().check_merge_next();
                    None
                } else {
                    Some(m)
                }
            }
        }
    }

    pub fn try_insert(&self, m: Memory<'heap>) -> Option<Memory<'heap>> {
        self.try_insert_before(m).and_then(|m| self.try_insert_after(m))
    }

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlockPtr<'heap> {
        unsafe { &mut *(self as *const FreeBlockPtr as *mut FreeBlockPtr) }
    }
}

impl<'heap> fmt::Debug for FreeBlockPtr<'heap> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.ptr)
    }
}


pub struct FreeBlock<'heap> {
    pub next: FreeBlockPtr<'heap>,
    pub size: usize,
}

pub const FREE_BLOCK_SIZE: usize = mem::size_of::<FreeBlock>();

impl<'heap> FreeBlock<'heap> {
    pub fn from_memory(m: Memory<'heap>, next: FreeBlockPtr<'heap>) -> &'heap mut FreeBlock<'heap> {
        let block = unsafe { &mut *(m.start() as *mut u8 as *mut FreeBlock) };
        block.next = next;
        block.size = m.len();
        block
    }

    pub fn as_memory(&self) -> Memory<'heap> {
        Memory::new(unsafe { slice::from_raw_parts_mut(self.start() as *mut u8, self.size) })
    }

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlock {
        unsafe { &mut *(self as *const FreeBlock as *mut FreeBlock) }
    }

    #[inline]
    pub fn start(&self) -> *mut u8 {
        self.as_mut() as *mut FreeBlock as *mut u8
    }

    #[inline]
    pub fn end(&self) -> *mut u8 {
        ((self.start() as usize) + self.size) as *mut u8
    }

    // check if this block and the next can be merged, and if so, merge them.
    pub fn check_merge_next(&mut self) {
        self.next.ptr.map(|next| {
            if self.end() == next.start() {
                self.size += next.size;
                self.next = next.next;
            }
        });
    }
}

impl<'heap> fmt::Debug for FreeBlock<'heap> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} @ {:?}", self.size, self as *const _)
    }
}


pub struct FreeListIterator<'a> {
    next: &'a FreeBlockPtr<'a>,
}

impl<'a> Iterator for FreeListIterator<'a> {
    type Item = &'a FreeBlock<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next.ptr.map(|block| {
            self.next = &block.next;
            block
        })
    }
}


// when traversing the free list, the correct place to insert may be the
// pointer _behind_ you. to prevent having to maintain a backwards pointer
// in the free list, we remember the previous pointer as we go.
#[derive(Clone, Copy)]
pub struct FreeListSpan<'a> {
    pub insert_point: &'a FreeBlockPtr<'a>,
    pub ptr: &'a FreeBlockPtr<'a>,
}

impl<'a> FreeListSpan<'a> {
    fn new(p: &'a FreeBlockPtr) -> FreeListSpan<'a> {
        FreeListSpan { insert_point: p, ptr: p }
    }

    // if you know for sure the memory will slot between these two free
    // blocks (in this span), we can do it O(1).
    pub fn insert(&self, m: Memory<'a>) {
        assert!(self.insert_point.try_insert_after(m).and_then(|m| self.ptr.try_insert_before(m)).is_none());
    }

    // you can traverse the free list as if this was an iterator.
    pub fn next(&self) -> Option<FreeListSpan<'a>> {
        self.ptr.ptr.map(|block| {
            FreeListSpan { insert_point: self.ptr, ptr: &block.next }
        })
    }
}


pub struct FreeListSpanIterator<'a> {
    next: Option<FreeListSpan<'a>>,
}

impl<'a> FreeListSpanIterator<'a> {
    fn new(p: &'a FreeBlockPtr<'a>) -> FreeListSpanIterator<'a> {
        FreeListSpanIterator { next: Some(FreeListSpan::new(p)) }
    }
}

impl<'a> Iterator for FreeListSpanIterator<'a> {
    type Item = FreeListSpan<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let rv = self.next;
        self.next = rv.and_then(|s| s.next());
        rv
    }
}


pub struct FreeList<'heap> {
    list: FreeBlockPtr<'heap>,
}

impl<'heap> FreeList<'heap> {
    pub fn new(m: Memory<'heap>) -> FreeList<'heap> {
        FreeList { list: FreeBlockPtr::new(m, LAST) }
    }

    pub fn iter(&self) -> FreeListIterator {
        FreeListIterator { next: &self.list }
    }

    // walk the free list, yielding both a FreeBlockPtr and an "insert point"
    // which will be the previous FreeBlockPtr or the initial one. the final
    // FreeBlockPtr will be a null pointer, so at least one FreeBlockPtr is
    // always yielded, even for an empty list.
    pub fn iter_span(&self) -> FreeListSpanIterator<'heap> {
        // FIXME: rust can't figure out that we're all "heap"-lifetime references
        FreeListSpanIterator::new(unsafe { mem::transmute(&self.list) })
    }

    #[cfg(test)]
    fn first_available(&self) -> *mut u8 {
        self.list.ptr.map(|block| block.start()).unwrap_or(core::ptr::null_mut())
    }

    pub fn allocate(&mut self, amount: usize) -> Option<Memory<'heap>> {
        self.iter_span().find_map(|p| p.ptr.allocate(amount))
    }

    pub fn retire(&mut self, m: Memory<'heap>) {
        // try_insert will return the memory if it won't fit here, so we
        // do some ✨shenanigans✨ to move the memory thru an option, so
        // rust will be satisfied.
        let mut mm = Some(m);
        assert!(self.iter_span().any(|span| {
            mm = span.ptr.try_insert(mm.take().unwrap());
            mm.is_none()
        }));
    }

    pub fn bytes(&self) -> usize {
        self.iter().map(|b| b.size).sum()
    }
}

impl<'heap> fmt::Debug for FreeList<'heap> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FreeList(")?;
        let mut first = true;
        for block in self.iter() {
            if !first { write!(f, " -> ")?; }
            first = false;
            write!(f, "{:?}", block)?;
        }
        write!(f, ")")
    }
}


#[cfg(test)]
mod tests {
    use super::{FreeList, Memory};

    fn assert_chain(f: &FreeList, expected: &[usize]) {
        let mut i = 0;
        for block in f.iter() {
            assert!(i < expected.len(), "{:?} != {:?}", f, expected);
            assert_eq!(expected[i], block.size, "{:?} != {:?}", f, expected);
            i += 1;
        }
        assert!(i == expected.len(), "{:?} != {:?}", f, expected);
    }

    fn assert_span_chain(f: &FreeList, expected: &[usize]) {
        let mut i = 0;
        for span in f.iter_span() {
            assert!(i < expected.len(), "{:?} != {:?}", f, expected);
            let size = span.ptr.ptr.map(|p| p.size).unwrap_or(0);
            assert_eq!(expected[i], size, "{:?} != {:?}", f, expected);
            i += 1;
        }
        assert!(i == expected.len(), "{:?} != {:?}", f, expected);
    }

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let start = &mut data[0] as *mut u8;
        let mut f = FreeList::new(Memory::new(&mut data));
        let origin = f.first_available();
        assert_eq!(origin, start);
        let alloc = f.allocate(120);
        assert!(alloc.is_some());
        if let Some(m) = alloc {
            assert_eq!(origin, m.start());
            assert_eq!(m.len(), 120);
        }
    }

    #[test]
    fn allocate_multiple() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Memory::new(&mut data));
        let origin = f.first_available();
        let m1 = f.allocate(64).unwrap();
        let m2 = f.allocate(32).unwrap();
        let m3 = f.allocate(32).unwrap();
        assert_eq!(m1.start(), origin);
        assert_eq!(m2.start(), m1.offset(64));
        assert_eq!(m3.start(), m1.offset(96));
        assert_eq!(f.first_available(), m1.offset(128));
    }

    #[test]
    fn allocate_to_exhaustion() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Memory::new(&mut data));
        let first_addr = f.first_available();
        let m1 = f.allocate(128).unwrap();
        let m2 = f.allocate(128).unwrap();
        let m3 = f.allocate(16);
        assert_eq!(m1.start(), first_addr);
        assert_eq!(m2.start(), m1.offset(128));
        assert!(m3.is_none());
        assert_chain(&f, &[]);
        assert_span_chain(&f, &[ 0 ]);
    }

    #[test]
    fn retire_first() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Memory::new(&mut data));
        let origin = f.first_available();
        let m1 = f.allocate(64);
        assert!(m1.is_some());
        if let Some(m) = m1 {
            f.retire(m);
            // the free block of 64 should have been merged back to the front
            // of the list as a single block.
            assert_chain(&f, &[ 256 ]);
            assert_span_chain(&f, &[ 256, 0 ]);
            assert_eq!(f.first_available(), origin);
        }
    }

    #[test]
    fn retire_last() {
        let mut data: [u8; 256] = [0; 256];
        let (m1, m2) = Memory::new(&mut data).split_at(128);
        let (_, m4) = m2.split_at(64);

        let mut f = FreeList::new(m1);
        let origin = f.first_available();
        f.retire(m4);
        assert_chain(&f, &[ 128, 64 ]);
        assert_span_chain(&f, &[ 128, 64, 0 ]);
        assert_eq!(f.first_available(), origin);
    }

    #[test]
    fn retire_middle() {
        let mut data: [u8; 256] = [0; 256];
        let (m1, m2) = Memory::new(&mut data).split_at(128);
        let (m3, m4) = m2.split_at(64);

        let mut f = FreeList::new(m1);
        let origin = f.first_available();
        f.retire(m4);
        assert_chain(&f, &[ 128, 64 ]);
        assert_eq!(f.first_available(), origin);

        f.retire(m3);
        assert_chain(&f, &[ 256 ]);
        assert_span_chain(&f, &[ 256, 0 ]);
        assert_eq!(f.first_available(), origin);
    }
}
