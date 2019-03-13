use core::slice;

/// Wrapper for an owned, mutable chunk of memory.
pub struct Memory<'heap>(&'heap mut [u8]);

impl<'heap> Memory<'heap> {
    /// Wrap a mutable slice of memory.
    pub fn new(m: &'heap mut [u8]) -> Memory {
        Memory(m)
    }

    /// Assuming you own a span of memory from the `start` address (inclusive)
    /// to the `end` address (exclusive), unsafely create a `Memory` wrapper
    /// for it.
    pub fn from_addresses(start: *mut u8, end: *mut u8) -> Memory<'heap> {
        Memory(unsafe { slice::from_raw_parts_mut(start, (end as usize) - (start as usize)) })
    }

    /// Equivalent to `slice::split_at_mut`.
    pub fn split_at(self, n: usize) -> (Memory<'heap>, Memory<'heap>) {
        let (m1, m2) = self.0.split_at_mut(n);
        (Memory(m1), Memory(m2))
    }

    /// Zero out this memory.
    pub fn clear(&mut self) {
        for i in 0..(self.0.len()) { self.0[i] = 0 }
    }

    /// Convert back into a mutable slice of memory, consuming this object.
    #[inline]
    pub fn inner(self) -> &'heap mut [u8] {
        self.0
    }

    /// Size (in bytes).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Starting address (inclusive).
    #[inline]
    pub fn start(&self) -> *mut u8 {
        self.0.as_ptr() as *mut u8
    }

    /// Ending address (exclusive).
    #[inline]
    pub fn end(&self) -> *mut u8 {
        ((self.start() as usize) + self.0.len()) as *mut u8
    }

    /// Address of memory within this span, offset `n` bytes.
    #[cfg(test)]
    pub fn offset(&self, n: usize) -> *mut u8 {
        ((self.start() as usize) + n) as *mut u8
    }
}
