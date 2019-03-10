use core::slice;
use crate::free_list::{FreeBlock, FreeBlockPtr};

// an owned piece of memory
pub struct Memory<'heap>(&'heap mut [u8]);

impl<'heap> Memory<'heap> {
    pub fn new(m: &'heap mut [u8]) -> Memory {
        Memory(m)
    }

    pub fn from_addresses(start: *const u8, end: *const u8) -> Memory<'heap> {
        Memory(unsafe { slice::from_raw_parts_mut(start as *mut u8, (end as usize) - (start as usize)) })
    }

    pub fn split_at(self, n: usize) -> (Memory<'heap>, Memory<'heap>) {
        let (m1, m2) = self.0.split_at_mut(n);
        (Memory(m1), Memory(m2))
    }

    // every block of memory is guaranteed to be big enough to hold a FreeBlock
    pub fn to_free_block(self, next: FreeBlockPtr<'heap>) -> &'heap mut FreeBlock {
        let block = unsafe { &mut *(self.0.as_ptr() as *mut u8 as *mut FreeBlock) };
        block.next = next;
        block.size = self.0.len();
        block
    }

    pub fn from_free_block(block: &'heap mut FreeBlock) -> Memory<'heap> {
        Memory(unsafe { slice::from_raw_parts_mut(block as *mut FreeBlock as *mut u8, block.size) })
    }

    pub fn clear(&mut self) {
        for i in 0..(self.0.len()) { self.0[i] = 0 }
    }

    #[inline]
    pub fn inner(self) -> &'heap mut [u8] {
        self.0
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn start(&self) -> *const u8 {
        self.0.as_ptr()
    }

    #[inline]
    pub fn end(&self) -> *const u8 {
        unsafe { self.start().offset(self.0.len() as isize) }
    }

    #[cfg(test)]
    pub fn offset(&self, n: isize) -> *const u8 {
        unsafe { self.start().offset(n) }
    }
}
