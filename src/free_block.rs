use core::fmt;
use core::mem::size_of;
use core::ptr;

// each free block is part of a linked list.

pub struct FreeBlock<'a> {
    pub next: Option<&'a FreeBlock<'a>>,
    pub size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();

impl<'a> FreeBlock<'a> {
    pub fn at<T>(p: *mut T) -> &'a mut FreeBlock<'a> {
        unsafe { &mut *(p as *mut FreeBlock) }
    }

    pub fn at_offset<T>(p: *mut T, offset: usize) -> &'a mut FreeBlock<'a> {
        unsafe { &mut *((p as *mut u8).offset(offset as isize) as *mut FreeBlock) }
    }

    pub fn as_mut(&self) -> &'a mut FreeBlock<'a> {
        unsafe { &mut *(self as *const FreeBlock as *mut FreeBlock) }
    }

    // split this free block, keeping `amount` in this one, and the remainder in a new linked block.
    pub fn split(&mut self, amount: usize) {
        assert!(amount <= self.size);
        assert!(amount >= FREE_BLOCK_SIZE && self.size - amount >= FREE_BLOCK_SIZE);
        let next = FreeBlock::at_offset(self as *mut _, amount);
        next.size = self.size - amount;
        next.next = self.next;
        self.size = amount;
        self.next = Some(next);
    }
}

impl<'a> fmt::Debug for FreeBlock<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} @ {:?}", self.size, self as *const _)?;
        match self.next {
            None => Ok(()),
            Some(next) => {
                write!(f, " -> ")?;
                next.fmt(f)
            }
        }
    }
}


pub struct FreeBlockIterator<'a> {
    current: Option<&'a FreeBlock<'a>>,
}

impl<'a> Iterator for FreeBlockIterator<'a> {
    type Item = &'a FreeBlock<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let rv = self.current;
        rv.map(|x| self.current = x.next);
        rv
    }
}
