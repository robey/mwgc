use core::fmt;
use core::mem::size_of;
use core::slice;

// each free block is part of a linked list.

#[derive(Clone, Copy, PartialEq)]
pub struct FreeBlockPtr {
    pub ptr: *const FreeBlock,
}

const END: FreeBlockPtr = FreeBlockPtr { ptr: 0 as *const FreeBlock };

impl FreeBlockPtr {
    pub fn at<T>(p: *const T, offset: isize) -> FreeBlockPtr {
        let ptr = unsafe { (p as *const u8).offset(offset) } as *const FreeBlock;
        FreeBlockPtr { ptr }
    }

    pub fn create_at<T>(p: *const T, offset: isize, next: *const FreeBlock, size: usize) -> FreeBlockPtr {
        let ptr = FreeBlockPtr::at(p, offset);
        ptr.block_mut().set(next, size);
        ptr
    }

    pub fn create(memory: &'static [u8], next: *const FreeBlock) -> FreeBlockPtr {
        let ptr = FreeBlockPtr::at(memory.as_ptr(), 0);
        ptr.block_mut().set(next, memory.len());
        ptr
    }

    pub fn block(&self) -> &'static FreeBlock {
        unsafe { &*self.ptr }
    }

    pub fn block_mut(&self) -> &'static mut FreeBlock {
        unsafe { &mut *(self.ptr as *mut FreeBlock) }
    }

    // returns true if it actually inserted. returns false if inserting here
    // would break the ordering.
    pub fn try_insert(&mut self, memory: &'static [u8]) -> bool {
        let block = self.block_mut();

        // if this is the end, append.
        if *self == END {
            self.ptr = FreeBlockPtr::create(memory, END.ptr).ptr;
            return true;
        }

        // insert before the current block.
        if block.start() > memory.as_ptr() {
            self.ptr = FreeBlockPtr::create(memory, self.ptr).ptr;
            self.block_mut().check_merge_next();
            return true;
        }

        // merge to the end of this block.
        if block.merge(memory) {
            block.check_merge_next();
            return true;
        }

        // if this block points to the end, append.
        if block.next == END {
            block.next = FreeBlockPtr::create(memory, END.ptr);
            return true;
        }

        false
    }
}

impl fmt::Debug for FreeBlockPtr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} @ {:?}", self.block().size, self.ptr)
    }
}


pub struct FreeBlock {
    pub next: FreeBlockPtr,
    pub size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();

impl FreeBlock {
    pub fn set(&mut self, next: *const FreeBlock, size: usize) {
        self.next.ptr = next;
        self.size = size;
    }

    pub fn start(&self) -> *const u8 {
        self as *const FreeBlock as *const u8
    }

    pub fn end(&self) -> *const u8 {
        unsafe { self.start().offset(self.size as isize) }
    }

    // split this free block, keeping `amount` in this one, and the remainder in a new linked block.
    pub fn split(&mut self, amount: usize) {
        assert!(amount <= self.size);
        assert!(amount >= FREE_BLOCK_SIZE && self.size - amount >= FREE_BLOCK_SIZE);

        let next = FreeBlockPtr::at(self, amount as isize);
        let next_block = next.block_mut();
        next_block.size = self.size - amount;
        next_block.next = FreeBlockPtr::at(self.next.ptr, 0);
        self.size = amount;
        self.next = next;
    }

    // attempt to allocate memory out of this block.
    // if it's possible, return the memory and a replacement FreeBlockPtr (this one is gone).
    pub fn allocate(&mut self, amount: usize) -> Option<(&'static [u8], FreeBlockPtr)> {
        if amount > self.size { return None }
        let memory = unsafe { slice::from_raw_parts(self as *const FreeBlock as *const u8, amount) };
        // if there isn't enough left in this block for a new block, just use it all.
        if self.size - amount < FREE_BLOCK_SIZE { return Some((memory, self.next)) }
        let next = FreeBlockPtr::create_at(self, amount as isize, self.next.ptr, self.size - amount);
        Some((memory, next))
    }

    // if `memory` follows this block sequentially, merge it in and return true.
    pub fn merge(&mut self, memory: &'static [u8]) -> bool {
        if self.end() == memory.as_ptr() {
            self.size += memory.len();
            true
        } else {
            false
        }
    }

    // check if this block and the next can be merged, and if so, merge them.
    pub fn check_merge_next(&mut self) {
        if self.next != END && self.end() == self.next.block().start() {
            let next = self.next.block();
            self.size += next.size;
            self.next = next.next;
        }
    }
}


pub struct FreeListIterator {
    current: FreeBlockPtr,
}

impl Iterator for FreeListIterator {
    type Item = FreeBlockPtr;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == END {
            None
        } else {
            let current = self.current;
            self.current = current.block().next;
            Some(current)
        }
    }
}


pub struct FreeListMutableIterator {
    current: *mut FreeBlockPtr,
}

impl Iterator for FreeListMutableIterator {
    type Item = &'static mut FreeBlockPtr;

    fn next(&mut self) -> Option<Self::Item> {
        // we know that the iteration is safe, but rust can't know that.
        let current = unsafe { &mut *self.current };
        if *current == END {
            None
        } else {
            self.current = &mut current.block_mut().next as *mut FreeBlockPtr;
            Some(current)
        }
    }
}


pub struct FreeList {
    pub list: FreeBlockPtr,
}

impl FreeList {
    pub fn new(memory: &'static [u8]) -> FreeList {
        FreeList { list: FreeBlockPtr::create(memory, END.ptr) }
    }

    // for tests:
    pub fn from_raw<T>(memory: &T, size: usize) -> FreeList {
        FreeList::new(unsafe { &*(slice::from_raw_parts(memory as *const T as *const u8, size)) })
    }

    pub fn iter(&self) -> FreeListIterator {
        FreeListIterator { current: self.list }
    }

    pub fn iter_mut(&mut self) -> FreeListMutableIterator {
        FreeListMutableIterator { current: &mut self.list as *mut FreeBlockPtr }
    }

    // for tests:
    pub fn first_available(&self) -> *const u8 {
        self.list.ptr as *const u8
    }

    // for tests:
    pub fn debug_chain(&self) -> Vec<usize> {
        self.iter().map(|p| p.block().size).collect::<Vec<usize>>()
    }

    pub fn allocate(&mut self, amount: usize) -> Option<&'static [u8]> {
        for ptr in self.iter_mut() {
            if let Some((memory, next)) = ptr.block_mut().allocate(amount) {
                ptr.ptr = next.ptr;
                return Some(memory);
            }
        }
        None
    }

    pub fn retire(&mut self, memory: &'static [u8]) {
        for ptr in self.iter_mut() {
            if ptr.try_insert(memory) { return }
        }
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
    use core::slice;
    use crate::FreeList;

    #[test]
    fn allocate() {
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 256);
        let origin = f.first_available();
        assert_eq!(origin, &data as *const u8);
        let alloc = f.allocate(120);
        assert!(alloc.is_some());
        if let Some(memory) = alloc {
            assert_eq!(origin, memory.as_ptr());
            assert_eq!(memory.len(), 120);
        }
    }

    #[test]
    fn allocate_multiple() {
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 256);
        let origin = f.first_available();
        let a1 = f.allocate(64);
        let a2 = f.allocate(32);
        let a3 = f.allocate(32);
        assert_eq!(a1.map(|a| a.as_ptr()), Some(origin));
        assert_eq!(a2.map(|a| a.as_ptr()), unsafe { a1.map(|a| a.as_ptr().offset(64)) });
        assert_eq!(a3.map(|a| a.as_ptr()), unsafe { a1.map(|a| a.as_ptr().offset(96)) });
        assert_eq!(Some(f.first_available()), unsafe { a1.map(|a| a.as_ptr().offset(128)) });
    }

    #[test]
    fn allocate_to_exhaustion() {
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 256);
        let first_addr = f.first_available();
        let a1 = f.allocate(128);
        let a2 = f.allocate(128);
        let a3 = f.allocate(16);
        assert_eq!(a1.map(|a| a.as_ptr()), Some(first_addr));
        assert_eq!(a2.map(|a| a.as_ptr()), unsafe { a1.map(|a| a.as_ptr().offset(128)) });
        assert_eq!(a3, None);
    }

    #[test]
    fn retire_first() {
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 256);
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
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 128);
        let origin = f.first_available();
        let a = unsafe { slice::from_raw_parts(data[192..].as_ptr(), 64) };
        f.retire(a);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.first_available(), origin);
    }

    #[test]
    fn retire_middle() {
        let data: [u8; 256] = [0; 256];
        let mut f = FreeList::from_raw(&data, 128);
        let origin = f.first_available();
        let a = unsafe { slice::from_raw_parts(data[192..].as_ptr(), 64) };
        f.retire(a);
        assert_eq!(f.debug_chain(), vec![ 128, 64 ]);
        assert_eq!(f.first_available(), origin);

        let b = unsafe { slice::from_raw_parts(data[128..].as_ptr(), 64) };
        f.retire(b);
        assert_eq!(f.debug_chain(), vec![ 256 ]);
        assert_eq!(f.first_available(), origin);
    }
}
