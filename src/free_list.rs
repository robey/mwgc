use core::fmt;
use core::mem::{size_of, swap};
use core::slice;

// each free block is part of a linked list.

// a span of memory
#[derive(Clone, Copy)]
pub struct Allocation {
    pub memory: &'static [u8],
}

impl Allocation {
    pub fn make<T>(obj: &T, size: usize) -> Allocation {
        Allocation { memory: unsafe { slice::from_raw_parts(obj as *const T as *const u8, size) } }
    }

    fn split_at(self, n: usize) -> (Allocation, Allocation) {
        let (m1, m2) = self.memory.split_at(n);
        (Allocation { memory: m1 }, Allocation { memory: m2 })
    }

    fn to_free_block(self, next: FreeBlockPtr) -> &'static mut FreeBlock {
        let block = unsafe { &mut *(self.memory.as_ptr() as *mut u8 as *mut FreeBlock) };
        block.next = next;
        block.size = self.memory.len();
        block
    }

    #[inline]
    pub fn start(&self) -> *const u8 {
        self.memory.as_ptr()
    }

    #[inline]
    pub fn end(&self) -> *const u8 {
        unsafe { self.memory.as_ptr().offset(self.memory.len() as isize) }
    }

    #[cfg(test)]
    pub fn offset(&self, n: isize) -> *const u8 {
        unsafe { self.start().offset(n) }
    }
}


// a FreeBlockPtr has "interior mutability"
#[derive(Clone, Copy)]
pub struct FreeBlockPtr {
    pub ptr: Option<&'static FreeBlock>,
}

const LAST: FreeBlockPtr = FreeBlockPtr { ptr: None };

impl FreeBlockPtr {
    pub fn new(a: Allocation, next: FreeBlockPtr) -> FreeBlockPtr {
        let block = a.to_free_block(next);
        FreeBlockPtr { ptr: Some(block) }
    }

    pub fn start(&self) -> Option<*const u8> {
        self.ptr.map(|p| p.start())
    }

    pub fn end(&self) -> Option<*const u8> {
        self.ptr.map(|p| p.end())
    }

    // attempt to allocate memory out of this block.
    pub fn allocate(&mut self, amount: usize) -> Option<Allocation> {
        self.ptr.and_then(|block| {
            if amount > block.size {
                None
            } else if block.size - amount < FREE_BLOCK_SIZE {
                // if there isn't enough left in this block for a new block, just use it all.
                self.ptr = block.next.ptr;
                Some(block.as_alloc())
            } else {
                // split off a new alloc
                let (a1, a2) = block.as_alloc().split_at(amount);
                self.ptr = Some(a2.to_free_block(block.next));
                Some(a1)
            }
        })
    }

    // returns true if it actually inserted. returns false if inserting here
    // would break the ordering.
    pub fn try_insert(&mut self, a: Allocation) -> bool {
        match self.ptr.as_mut() {
            None => {
                // if this is the end, append.
                self.ptr = Some(a.to_free_block(LAST));
                true
            },
            Some(block) => {
                if block.start() > a.memory.as_ptr() {
                    // insert before the current block.
                    let new_block = a.to_free_block(*self);
                    new_block.check_merge_next();
                    self.ptr = Some(new_block);
                    true
                } else if block.end() == a.memory.as_ptr() {
                    // merge to the end of this block.
                    block.as_mut().size += a.memory.len();
                    block.as_mut().check_merge_next();
                    true
                } else {
                    false
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
    fn as_alloc(&self) -> Allocation {
        Allocation::make(self, self.size)
    }

    // for internal mutations only
    fn as_mut(&self) -> &mut FreeBlock {
        unsafe { &mut *(self as *const FreeBlock as *mut FreeBlock) }
    }

    pub fn start(&self) -> *const u8 {
        self.as_alloc().start()
    }

    pub fn end(&self) -> *const u8 {
        self.as_alloc().end()
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
    pub fn new(alloc: Allocation) -> FreeList {
        FreeList { list: FreeBlockPtr::new(alloc, LAST) }
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
        self.list.ptr.map(|block| block.as_alloc().start()).unwrap_or(0 as *const u8)
    }

    #[cfg(test)]
    fn debug_chain(&self) -> Vec<usize> {
        self.iter().map(|p| p.size).collect::<Vec<usize>>()
    }

    pub fn allocate(&mut self, amount: usize) -> Option<Allocation> {
        self.iter_mut().find_map(|p| p.allocate(amount))
    }

    pub fn retire(&mut self, a: Allocation) {
        assert!(self.iter_mut().any(|p| p.try_insert(a)));
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
    use super::{Allocation, FreeList};

    #[test]
    fn allocate() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 256));
        let origin = f.first_available();
        assert_eq!(origin, &data as *const u8);
        let alloc = f.allocate(120);
        assert!(alloc.is_some());
        if let Some(a) = alloc {
            assert_eq!(origin, a.memory.as_ptr());
            assert_eq!(a.memory.len(), 120);
        }
    }

    #[test]
    fn allocate_multiple() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 256));
        let origin = f.first_available();
        let a1 = f.allocate(64).unwrap();
        let a2 = f.allocate(32).unwrap();
        let a3 = f.allocate(32).unwrap();
        assert_eq!(a1.start(), origin);
        assert_eq!(a2.start(), a1.offset(64));
        assert_eq!(a3.start(), a1.offset(96));
        assert_eq!(f.first_available(), a1.offset(128));
    }

    #[test]
    fn allocate_to_exhaustion() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 256));
        let first_addr = f.first_available();
        let a1 = f.allocate(128).unwrap();
        let a2 = f.allocate(128).unwrap();
        let a3 = f.allocate(16);
        assert_eq!(a1.start(), first_addr);
        assert_eq!(a2.start(), a1.offset(128));
        assert!(a3.is_none());
    }

    #[test]
    fn retire_first() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 256));
        let origin = f.first_available();
        let a1 = f.allocate(64);
        assert!(a1.is_some());
        if let Some(a) = a1 {
            f.retire(a);
            // the free block of 64 should have been merged back to the front
            // of the list as a single block.
            assert_eq!(f.debug_chain(), vec![ 256 ]);
            assert_eq!(f.first_available(), origin);
        }
    }

    #[test]
    fn retire_last() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 128));
        let origin = f.first_available();
        let a = Allocation::make(&data[192], 64);
        f.retire(a);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.first_available(), origin);
    }

    #[test]
    fn retire_middle() {
        let mut data: [u8; 256] = [0; 256];
        let mut f = FreeList::new(Allocation::make(&mut data, 128));
        let origin = f.first_available();
        let a = Allocation::make(&data[192], 64);
        f.retire(a);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.first_available(), origin);

        let b = Allocation::make(&data[128], 64);
        f.retire(b);
        assert_eq!(f.debug_chain(), vec![ 256 ]);
        assert_eq!(f.first_available(), origin);
    }
}
