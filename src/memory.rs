use core::slice;
use crate::free_list::{FreeBlock, FreeBlockPtr};

// an owned piece of memory
pub struct Memory(&'static mut [u8]);

impl Memory {
    pub fn new(m: &'static mut [u8]) -> Memory {
        Memory(m)
    }

    pub fn take(m: &mut [u8]) -> Memory {
        Memory(unsafe { &mut *(m as *mut [u8]) })
    }

    pub fn make<T>(obj: &mut T, size: usize) -> Memory {
        Memory(unsafe { slice::from_raw_parts_mut(obj as *mut T as *mut u8, size) })
    }

    pub fn split_at(self, n: usize) -> (Memory, Memory) {
        let (m1, m2) = self.0.split_at_mut(n);
        (Memory(m1), Memory(m2))
    }

    // every block of memory is guaranteed to be big enough to hold a FreeBlock
    pub fn to_free_block(self, next: FreeBlockPtr) -> &'static mut FreeBlock {
        let block = unsafe { &mut *(self.0.as_ptr() as *mut u8 as *mut FreeBlock) };
        block.next = next;
        block.size = self.0.len();
        block
    }

    #[inline]
    pub fn inner(self) -> &'static mut [u8] {
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
