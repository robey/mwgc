use core::fmt;
use core::mem::{size_of, swap};
use crate::memory::Memory;

// each free block is part of a linked list.


// a FreeBlockPtr has "interior mutability"
#[derive(Clone, Copy)]
pub struct FreeBlockPtr {
    pub ptr: Option<&'static FreeBlock>,
}

const LAST: FreeBlockPtr = FreeBlockPtr { ptr: None };

impl FreeBlockPtr {
    pub fn new(m: Memory, next: FreeBlockPtr) -> FreeBlockPtr {
        let block = m.to_free_block(next);
        FreeBlockPtr { ptr: Some(block) }
    }

    pub fn start(&self) -> Option<*const u8> {
        self.ptr.map(|p| p.start())
    }

    pub fn end(&self) -> Option<*const u8> {
        self.ptr.map(|p| p.end())
    }

    pub fn size(&self) -> Option<usize> {
        self.ptr.map(|p| p.size)
    }

    pub fn next(&self) -> Option<&FreeBlockPtr> {
        self.ptr.map(|p| &p.next)
    }

    // attempt to allocate memory out of this block.
    pub fn allocate(&mut self, amount: usize) -> Option<Memory> {
        self.ptr.and_then(|block| {
            if amount > block.size {
                None
            } else if block.size - amount < FREE_BLOCK_SIZE {
                // if there isn't enough left in this block for a new block, just use it all.
                self.ptr = block.next.ptr;
                Some(block.as_mut().to_memory())
            } else {
                // split off a new alloc
                let (a1, a2) = block.as_mut().to_memory().split_at(amount);
                self.ptr = Some(a2.to_free_block(block.next));
                Some(a1)
            }
        })
    }

    // the inserts will consume the memory if it was successfully inserted,
    // or return it if this isn't the right place.

    pub fn try_insert_before(&self, m: Memory) -> Option<Memory> {
        let s = self.as_mut();
        if let Some(block) = s.ptr.as_mut() {
            if block.start() > m.start() {
                // insert before the current block.
                let new_block = m.to_free_block(*self);
                new_block.check_merge_next();
                s.ptr = Some(new_block);
                return None
            }
        }
        Some(m)
    }

    pub fn try_insert_after(&self, m: Memory) -> Option<Memory> {
        let s = self.as_mut();
        match s.ptr.as_mut() {
            None => {
                // if this is the end, append.
                s.ptr = Some(m.to_free_block(LAST));
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

    // consumes the memory if it actually inserted. returns the memory if
    // inserting here would break the ordering.
    pub fn try_insert(&self, m: Memory) -> Option<Memory> {
        self.try_insert_before(m).and_then(|m| self.try_insert_after(m))
    }

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlockPtr {
        unsafe { &mut *(self as *const FreeBlockPtr as *mut FreeBlockPtr) }
    }
}

impl fmt::Debug for FreeBlockPtr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.ptr)
    }
}


pub struct FreeBlock {
    pub next: FreeBlockPtr,
    pub size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();

impl FreeBlock {
    // presto-chango back to usable memory!
    fn to_memory(&mut self) -> Memory {
        Memory::make(self, self.size)
    }

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlock {
        unsafe { &mut *(self as *const FreeBlock as *mut FreeBlock) }
    }

    #[inline]
    pub fn start(&self) -> *const u8 {
        self as *const FreeBlock as *const u8
    }

    #[inline]
    pub fn end(&self) -> *const u8 {
        ((self.start() as usize) + self.size) as *const u8
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

impl fmt::Debug for FreeBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} @ {:?}", self.size, self as *const _)
    }
}


pub struct FreeListIterator<'a> {
    next: &'a FreeBlockPtr,
}

impl<'a> Iterator for FreeListIterator<'a> {
    type Item = &'a FreeBlock;

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
    pub insert_point: &'a FreeBlockPtr,
    pub ptr: &'a FreeBlockPtr,
}

impl<'a> FreeListSpan<'a> {
    pub fn new(p: &'a FreeBlockPtr) -> FreeListSpan<'a> {
        FreeListSpan { insert_point: p, ptr: p }
    }

    // if you know for sure the memory will slot between these two free
    // blocks (in this span), we can do it O(1).
    pub fn insert(&self, m: Memory) {
        assert!(self.insert_point.try_insert_after(m).and_then(|m| self.ptr.try_insert_before(m)).is_none());
    }

    // you can traverse the free list as if this was an iterator.
    pub fn next(&self) -> Option<FreeListSpan<'a>> {
        self.ptr.ptr.map(|block| {
            FreeListSpan { insert_point: self.ptr, ptr: &block.next }
        })
    }
}


pub struct FreeListMutableIterator<'a> {
    current: Option<&'a mut FreeBlockPtr>,
}

impl<'a> FreeListMutableIterator<'a> {
    // add memory to the free list, starting from the FreeBlockPtr that
    // would be emitted next. if it can be inserted here, the iterator will
    // be moved up so that the next emitted FreeBlockPtr will be where it was
    // inserted. (this is used by sweep, which has a list of memory spans
    // to insert, pre-sorted. it can use this to insert them all in O(n).)
    pub fn retire(&mut self, m: Memory) -> bool {
        // try_insert will return the memory if it won't fit here, so we
        // do some ✨shenanigans✨ to move the memory thru an option, so
        // rust will be satisfied.
        let mut mm = Some(m);
        while let Some(ptr) = &mut self.current {
            let m = mm.take().unwrap();
            mm = (*ptr).try_insert(m);
            if mm.is_none() { return true }
            self.next();
        }
        false
    }
}

impl<'a> Iterator for FreeListMutableIterator<'a> {
    type Item = &'a mut FreeBlockPtr;

    fn next(&mut self) -> Option<Self::Item> {
        match &self.current {
            None => None,
            Some(current) => {
                // sleight of hand, to satisfy the borrow checker
                let mut next = current.ptr.map(|b| b.next.as_mut());
                swap(&mut next, &mut self.current);
                next
            }
        }
    }
}


pub struct FreeList {
    list: FreeBlockPtr,
}

impl FreeList {
    pub fn new(m: Memory) -> FreeList {
        FreeList { list: FreeBlockPtr::new(m, LAST) }
    }

    pub fn iter(&self) -> FreeListIterator {
        FreeListIterator { next: &self.list }
    }

    // walk the free list, yielding both a FreeBlockPtr and an "insert point"
    // which will be the previous FreeBlockPtr or the initial one. the final
    // FreeBlockPtr will be a null pointer, so at least one FreeBlockPtr is
    // always yielded, even for an empty list.
    pub fn iter_span(&self) -> FreeListSpan {
        FreeListSpan::new(&self.list)
    }

    pub fn iter_mut(&mut self) -> FreeListMutableIterator {
        FreeListMutableIterator { current: Some(&mut self.list) }
    }

    pub fn first(&self) -> FreeBlockPtr {
        self.list
    }

    pub fn first_ref(&self) -> &FreeBlockPtr {
        &self.list
    }

    #[cfg(test)]
    fn first_available(&self) -> *const u8 {
        self.list.ptr.map(|block| block.start()).unwrap_or(core::ptr::null())
    }

    #[cfg(test)]
    fn debug_chain(&self) -> Vec<usize> {
        self.iter().map(|p| p.size).collect::<Vec<usize>>()
    }

    #[cfg(test)]
    fn debug_span_chain(&self) -> Vec<usize> {
        let mut v = Vec::<usize>::new();
        let mut span = self.iter_span();
        loop {
            v.push(span.ptr.size().unwrap_or(0));
            let next = span.next();
            if next.is_none() { return v }
            span = next.unwrap();
        }
    }

    pub fn allocate(&mut self, amount: usize) -> Option<Memory> {
        self.iter_mut().find_map(|p| p.allocate(amount))
    }

    pub fn retire(&mut self, m: Memory) {
        assert!(self.iter_mut().retire(m));
    }
}

impl fmt::Debug for FreeList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "FreeList({})", self.iter().map(|block| {
            format!("{:?}", block)
        }).collect::<Vec<String>>().join(" -> "))
    }
}


#[cfg(test)]
mod tests {
    use super::{FreeList, Memory};

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Memory::take(&mut data));
        let origin = f.first_available();
        assert_eq!(origin, &data as *const u8);
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
        let mut f = FreeList::new(Memory::take(&mut data));
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
        let mut f = FreeList::new(Memory::take(&mut data));
        let first_addr = f.first_available();
        let m1 = f.allocate(128).unwrap();
        let m2 = f.allocate(128).unwrap();
        let m3 = f.allocate(16);
        assert_eq!(m1.start(), first_addr);
        assert_eq!(m2.start(), m1.offset(128));
        assert!(m3.is_none());
        assert_eq!(f.debug_chain(), vec![]);
        assert_eq!(f.debug_span_chain(), vec![ 0 ]);
    }

    #[test]
    fn retire_first() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Memory::take(&mut data));
        let origin = f.first_available();
        let m1 = f.allocate(64);
        assert!(m1.is_some());
        if let Some(m) = m1 {
            f.retire(m);
            // the free block of 64 should have been merged back to the front
            // of the list as a single block.
            assert_eq!(f.debug_chain(), vec![ 256 ]);
            assert_eq!(f.debug_span_chain(), vec![ 256, 0 ]);
            assert_eq!(f.first_available(), origin);
        }
    }

    #[test]
    fn retire_last() {
        let mut data: [u8; 256] = [0; 256];
        let (m1, m2) = Memory::take(&mut data).split_at(128);
        let (_, m4) = m2.split_at(64);

        let mut f = FreeList::new(m1);
        let origin = f.first_available();
        f.retire(m4);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.debug_span_chain(), vec![ 128, 64, 0 ]);
        assert_eq!(f.first_available(), origin);
    }

    #[test]
    fn retire_middle() {
        let mut data: [u8; 256] = [0; 256];
        let (m1, m2) = Memory::take(&mut data).split_at(128);
        let (m3, m4) = m2.split_at(64);

        let mut f = FreeList::new(m1);
        let origin = f.first_available();
        f.retire(m4);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.first_available(), origin);

        f.retire(m3);
        assert_eq!(f.debug_chain(), vec![ 256 ]);
        assert_eq!(f.debug_span_chain(), vec![ 256, 0 ]);
        assert_eq!(f.first_available(), origin);
    }
}
