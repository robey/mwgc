use core::fmt;
use core::mem::size_of;
use core::ptr;

// each free block is part of a linked list.
pub struct FreeBlock {
    next: *mut FreeBlock,
    size: usize,
}

pub const FREE_BLOCK_SIZE: usize = size_of::<FreeBlock>();

// end of the linked list
const END: *mut FreeBlock = ptr::null_mut();

impl FreeBlock {
    pub fn init(free: *mut FreeBlock, size: usize) {
        unsafe {
            (*free).next = END;
            (*free).size = size;
        }
    }

    pub fn size(free: *mut FreeBlock) -> usize {
        unsafe { (*free).size }
    }

    pub fn next(free: *mut FreeBlock) -> *mut FreeBlock {
        if free == END {
            END
        } else {
            unsafe { (*free).next }
        }
    }

    pub fn link(a: *mut FreeBlock, b: *mut FreeBlock) {
        unsafe { (*a).next = b; }
    }

    pub fn display(free: *mut FreeBlock, f: &mut fmt::Formatter) -> fmt::Result {
        if free == END {
            return Ok(())
        }
        write!(f, "{} @ {:?}", FreeBlock::size(free), free)?;

        let mut p = FreeBlock::next(free);
        while p != END {
            write!(f, "-> {} @ {:?}", FreeBlock::size(p), p)?;
            p = FreeBlock::next(p);
        }
        Ok(())
    }
}


pub struct FreeBlockIterator {
    start: *mut FreeBlock,
    next: *mut FreeBlock,
}
