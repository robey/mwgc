use core::{fmt, str};

/// a way to write strings for debugging, without having the stdlib
pub struct StringBuffer<'a> {
    buffer: &'a mut [u8],
    index: usize,
}

impl<'a> StringBuffer<'a> {
    pub fn new(buffer: &'a mut [u8]) -> StringBuffer<'a> {
        StringBuffer { buffer, index: 0 }
    }

    pub fn to_str(self) -> &'a str {
        str::from_utf8(&self.buffer[0 .. self.index]).unwrap()
    }
}

impl<'a> fmt::Write for StringBuffer<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.index + bytes.len() > self.buffer.len() { return Err(core::fmt::Error); }
        for i in 0 .. bytes.len() { self.buffer[self.index + i] = bytes[i]; }
        self.index += bytes.len();
        Ok(())
    }
}
