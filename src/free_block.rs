use core::fmt;
use core::mem::size_of;
use core::ptr;

// each free block is part of a linked list.

pub struct FreeBlockLink {
    link: *mut FreeBlock,
}

pub struct FreeBlock {
    next: FreeBlockLink,
    size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();

impl FreeBlockLink {
    pub fn at<T>(p: *mut T) -> FreeBlockLink {
        FreeBlockLink { link: p as *mut FreeBlock }
    }

    // end of the linked list
    pub fn end() -> FreeBlockLink {
        FreeBlockLink { link: ptr::null_mut() }
    }

    pub fn init(&mut self, size: usize) -> &FreeBlockLink {
        unsafe {
            (*self.link).next = FreeBlockLink::end();
            (*self.link).size = size;
        }
        self
    }

    pub fn is_end(&self) -> bool {
        self.link == ptr::null_mut()
    }

    pub fn size(&self) -> usize {
        unsafe { (*self.link).size }
    }

    pub fn next(&self) -> &FreeBlockLink {
        if self.is_end() {
            self
        } else {
            unsafe { &(*self.link).next }
        }
    }

    pub fn link(&mut self, b: FreeBlockLink) {
        self.link = b.link;
    }
}

impl fmt::Debug for FreeBlockLink {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_end() { return Ok(()) }
        write!(f, "{} @ {:?}", self.size(), self.link)?;
        if self.next().is_end() { return Ok(()) }
        write!(f, " -> ")?;
        self.next().fmt(f)
    }
}


pub struct FreeBlockIterator {
    start: *mut FreeBlock,
    next: *mut FreeBlock,
}
