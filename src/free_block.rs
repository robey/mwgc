use core::fmt;
use core::mem::size_of;

// each free block is part of a linked list.

pub type FreeBlockRef = &'static FreeBlock;

pub struct FreeBlock {
    pub next: Option<FreeBlockRef>,
    pub size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();


impl FreeBlock {
    pub fn at<T>(p: *const T, offset: usize) -> FreeBlockRef {
        unsafe { &*((p as *const u8).offset(offset as isize) as *const FreeBlock) }
    }

    pub fn as_mut(&self) -> &'static mut FreeBlock {
        unsafe { &mut *(self as *const FreeBlock as *mut FreeBlock) }
    }

    // split this free block, keeping `amount` in this one, and the remainder in a new linked block.
    pub fn split(&mut self, amount: usize) {
        assert!(amount <= self.size);
        assert!(amount >= FREE_BLOCK_SIZE && self.size - amount >= FREE_BLOCK_SIZE);
        let mut next = FreeBlock::at(self, amount).as_mut();
        next.size = self.size - amount;
        next.next = self.next;
        self.size = amount;
        self.next = Some(next);
    }
}

impl fmt::Debug for FreeBlock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} @ {:?}", self.size, self as *const _)
    }
}


pub struct FreeListIterator {
    current: Option<FreeBlockRef>,
}

impl Iterator for FreeListIterator {
    type Item = FreeBlockRef;

    fn next(&mut self) -> Option<Self::Item> {
        let rv = self.current;
        rv.map(|x| self.current = x.next);
        rv
    }
}


pub struct FreeList {
    pub list: FreeBlockRef,
}

impl FreeList {
    pub fn new(location: *const u8, size: usize) -> FreeList {
        let mut block = FreeBlock::at(location, 0).as_mut();
        block.size = size;
        block.next = None;
        FreeList { list: block }
    }

    pub fn iter(&self) -> FreeListIterator {
        FreeListIterator { current: Some(self.list) }
    }
}

impl fmt::Debug for FreeList {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.iter().map(|block| format!("{:?}", block)).collect::<Vec<String>>().join(" -> "))
    }
}
