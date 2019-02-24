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

    // returns true if it actually inserted. returns the memory if inserting here
    // would break the ordering.
    pub fn try_insert(&mut self, m: Memory) -> Option<Memory> {
        match self.ptr.as_mut() {
            None => {
                // if this is the end, append.
                self.ptr = Some(m.to_free_block(LAST));
                None
            },
            Some(block) => {
                if block.start() > m.start() {
                    // insert before the current block.
                    let new_block = m.to_free_block(*self);
                    new_block.check_merge_next();
                    self.ptr = Some(new_block);
                    None
                } else if block.end() == m.start() {
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

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlockPtr {
        unsafe { &mut *(self as *const FreeBlockPtr as *mut FreeBlockPtr) }
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
    current: &'a FreeBlockPtr,
}

impl<'a> Iterator for FreeListIterator<'a> {
    type Item = &'a FreeBlock;

    fn next(&mut self) -> Option<Self::Item> {
        match self.current.ptr {
            None => None,
            Some(block) => {
                self.current = &block.next;
                Some(block)
            }
        }
    }
}


pub struct FreeListMutableIterator<'a> {
    current: Option<&'a mut FreeBlockPtr>,
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
        FreeListIterator { current: &self.list }
    }

    pub fn iter_mut(&mut self) -> FreeListMutableIterator {
        FreeListMutableIterator { current: Some(&mut self.list) }
    }

    pub fn first(&self) -> FreeBlockPtr {
        self.list
    }

    #[cfg(test)]
    fn first_available(&self) -> *const u8 {
        self.list.ptr.map(|block| block.start()).unwrap_or(0 as *const u8)
    }

    #[cfg(test)]
    fn debug_chain(&self) -> Vec<usize> {
        self.iter().map(|p| p.size).collect::<Vec<usize>>()
    }

    pub fn allocate(&mut self, amount: usize) -> Option<Memory> {
        self.iter_mut().find_map(|p| p.allocate(amount))
    }

    pub fn retire(&mut self, m: Memory) {
        // try_insert will return the memory if it won't fit here, so we
        // do some ✨shenanigans✨ to move the memory thru an option, so
        // rust will be satisfied.
        let mut mm = Some(m);
        assert!(self.iter_mut().any(|p| {
            let m = mm.take().unwrap();
            mm = p.try_insert(m);
            mm.is_none()
        }));
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
            assert_eq!(f.first_available(), origin);
        }
    }

    #[test]
    fn retire_last() {
        let mut data: [u8; 256] = [0; 256];
        let (m1, m2) = Memory::take(&mut data).split_at(128);
        let (m3, m4) = m2.split_at(64);

        let mut f = FreeList::new(m1);
        let origin = f.first_available();
        f.retire(m4);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
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
        assert_eq!(f.first_available(), origin);
    }
}
