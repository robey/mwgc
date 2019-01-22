use core::fmt;
use core::mem::size_of;
use core::slice;

// each free block is part of a linked list.

#[derive(Clone, Copy, PartialEq)]
pub struct FreeBlockPtr {
    pub ptr: *const FreeBlock,
}

const END: FreeBlockPtr = FreeBlockPtr {  ptr: 0 as *const FreeBlock };

impl FreeBlockPtr {
    pub fn at<T>(p: *const T, offset: isize) -> FreeBlockPtr {
        let ptr = unsafe { (p as *const u8).offset(offset) } as *const FreeBlock;
        FreeBlockPtr { ptr }
    }

    pub fn create<T>(p: *const T, offset: isize, next: FreeBlockPtr, size: usize) -> FreeBlockPtr {
        let ptr = FreeBlockPtr::at(p, offset);
        ptr.block_mut().set(next, size);
        ptr
    }

    pub fn block(&self) -> &'static FreeBlock {
        unsafe { &*self.ptr }
    }

    pub fn block_mut(&self) -> &'static mut FreeBlock {
        unsafe { &mut *(self.ptr as *mut FreeBlock) }
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
    pub fn set(&mut self, next: FreeBlockPtr, size: usize) {
        self.next = next;
        self.size = size;
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
        let next = FreeBlockPtr::create(self, amount as isize, self.next, self.size - amount);
        Some((memory, next))
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
    pub fn new(location: *const u8, size: usize) -> FreeList {
        let ptr = FreeBlockPtr::at(location, 0);
        ptr.block_mut().size = size;
        ptr.block_mut().next = END;
        FreeList { list: ptr }
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

    pub fn allocate(&mut self, amount: usize) -> Option<&'static [u8]> {
        for ptr in self.iter_mut() {
            if let Some((memory, next)) = ptr.block_mut().allocate(amount) {
                ptr.ptr = next.ptr;
                return Some(memory);
            }
        }
        None
    }
}

impl fmt::Debug for FreeList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.iter().map(|block| format!("{:?}", block)).collect::<Vec<String>>().join(" -> "))
    }
}
