use core::fmt;
use core::mem;
use crate::memory::Memory;

// we need to reserve 2 bits per block for tracking.
pub const BLOCKS_PER_COLORMAP_BYTE: usize = 8 / 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Continue = 0b00,
    Blue = 0b01,
    Green = 0b10,
    Check = 0b11,
}

impl Color {
    // why isn't this automatic or derivable?
    pub fn from_bits(n: u8) -> Color {
        unsafe { mem::transmute(n) }
    }

    pub fn opposite(&self) -> Color {
        match *self {
            Color::Blue => Color::Green,
            Color::Green => Color::Blue,
            x => x
        }
    }
}


// units are block numbers, starting from 0
#[derive(Debug, PartialEq)]
pub struct BlockRange {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}


// the color map sits at the end of the heap, and uses 2 bits to describe the
// color of each memory block. each range of allocated memory starts with a
// color (blue, green, or gray) and is followed by zero or more blocks that
// are marked as "continue".
// (free memory is tracked separately on a sorted FreeList.)
pub struct ColorMap<'heap> {
    bits: &'heap mut [u8],
}

impl<'heap> ColorMap<'heap> {
    pub fn new(m: Memory<'heap>) -> ColorMap<'heap> {
        let bits = m.inner();
        // mark whole area as "free" (check)
        for i in 0..bits.len() { bits[i] = 0xff }
        ColorMap { bits }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.bits.len() * BLOCKS_PER_COLORMAP_BYTE
    }

    pub fn get(&self, n: usize) -> Color {
        let shift = (n & 3) * 2;
        let mask = 3 << shift;
        Color::from_bits((self.bits[n / 4] & mask) >> shift)
    }

    pub fn set(&mut self, n: usize, color: Color) {
        let shift = (n & 3) * 2;
        let mask = !(3 << shift);
        let replace = (color as u8) << shift;
        self.bits[n / 4] = (self.bits[n / 4] & mask) | replace;
    }

    pub fn get_range(&self, n: usize) -> BlockRange {
        let color = self.get(n);
        let mut end = n + 1;
        while end < self.len() && self.get(end) == Color::Continue { end += 1 }
        BlockRange { start: n, end, color }
    }

    pub fn set_range(&mut self, range: BlockRange) {
        self.set(range.start, range.color);
        for i in (range.start + 1)..(range.end) { self.set(i, Color::Continue) }
    }

    // free ranges must be marked with a run of "check" so they can terminate a previous range
    pub fn free_range(&mut self, range: BlockRange) {
        for i in (range.start)..(range.end) { self.set(i, Color::Check) }
    }

    pub fn iter(&self) -> ColorMapIterator {
        ColorMapIterator { color_map: self, current: 0 }
    }
}

impl<'heap> fmt::Debug for ColorMap<'heap> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ColorMap(")?;
        for i in 0..(self.bits.len() * 4) {
            write!(f, "{}", match self.get(i) {
                Color::Blue => "B",
                Color::Green => "G",
                Color::Check => "C",
                Color::Continue => ".",
            })?;
        }
        write!(f, ")")
    }
}


pub struct ColorMapIterator<'a> {
    color_map: &'a ColorMap<'a>,
    current: usize,
}

impl<'a> Iterator for ColorMapIterator<'a> {
    type Item = BlockRange;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.color_map.len() { return None }
        let range = self.color_map.get_range(self.current);
        self.current = range.end;
        Some(range)
    }
}


#[cfg(test)]
mod tests {
    use crate::{BlockRange, Color, ColorMap};
    use crate::memory::Memory;

    #[test]
    fn init() {
        let mut data: [u8; 4] = [0; 4];
        let map = ColorMap::new(Memory::new(&mut data));
        assert_eq!(format!("{:?}", map), "ColorMap(CCCCCCCCCCCCCCCC)");
    }

    #[test]
    fn set_and_get_ranges() {
        let mut data: [u8; 4] = [0; 4];
        let mut map = ColorMap::new(Memory::new(&mut data));
        map.set_range(BlockRange { start: 0, end: 2, color: Color::Green });
        assert_eq!(format!("{:?}", map), "ColorMap(G.CCCCCCCCCCCCCC)");
        assert_eq!(map.get_range(0), BlockRange { start: 0, end: 2, color: Color::Green });

        map.set_range(BlockRange { start: 2, end: 3, color: Color::Blue });
        assert_eq!(map.get_range(2), BlockRange { start: 2, end: 3, color: Color::Blue });
        assert_eq!(map.get_range(0), BlockRange { start: 0, end: 2, color: Color::Green });
        assert_eq!(format!("{:?}", map), "ColorMap(G.BCCCCCCCCCCCCC)");
    }
}
