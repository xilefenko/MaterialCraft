//! Bounds-checked cursor over a byte slice. Every read either succeeds or returns [`OutOfBounds`];
//! nothing here panics on malformed input.

use std::fmt;

/// A read went past the end of the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfBounds {
    pub at: usize,
    pub wanted: usize,
    pub len: usize,
}

impl fmt::Display for OutOfBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "wanted {} bytes at offset {}, but data is {} bytes long",
            self.wanted, self.at, self.len
        )
    }
}

impl std::error::Error for OutOfBounds {}

#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

macro_rules! read_num {
    ($name:ident, $ty:ty, $from:ident) => {
        pub fn $name(&mut self) -> Result<$ty, OutOfBounds> {
            let b = self.bytes(std::mem::size_of::<$ty>())?;
            Ok(<$ty>::$from(
                b.try_into().expect("slice length matches type size"),
            ))
        }
    };
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        ByteReader { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn is_at_end(&self) -> bool {
        self.pos == self.data.len()
    }

    pub fn seek(&mut self, pos: usize) -> Result<(), OutOfBounds> {
        if pos > self.data.len() {
            return Err(OutOfBounds {
                at: pos,
                wanted: 0,
                len: self.data.len(),
            });
        }
        self.pos = pos;
        Ok(())
    }

    pub fn skip(&mut self, n: usize) -> Result<(), OutOfBounds> {
        self.bytes(n).map(|_| ())
    }

    /// Next `n` bytes, borrowed from the underlying data.
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], OutOfBounds> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len());
        match end {
            Some(end) => {
                let out = &self.data[self.pos..end];
                self.pos = end;
                Ok(out)
            }
            None => Err(OutOfBounds {
                at: self.pos,
                wanted: n,
                len: self.data.len(),
            }),
        }
    }

    /// Everything from the current position to the end.
    pub fn rest(&mut self) -> &'a [u8] {
        let out = &self.data[self.pos..];
        self.pos = self.data.len();
        out
    }

    pub fn u8(&mut self) -> Result<u8, OutOfBounds> {
        Ok(self.bytes(1)?[0])
    }

    read_num!(u16_le, u16, from_le_bytes);
    read_num!(u32_le, u32, from_le_bytes);
    read_num!(u64_le, u64, from_le_bytes);
    read_num!(i32_le, i32, from_le_bytes);
    read_num!(f32_le, f32, from_le_bytes);
    read_num!(u16_be, u16, from_be_bytes);
    read_num!(u32_be, u32, from_be_bytes);
    read_num!(i32_be, i32, from_be_bytes);
    read_num!(u64_be, u64, from_be_bytes);
    read_num!(i64_be, i64, from_be_bytes);
    read_num!(f64_be, f64, from_be_bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_byte_orders() {
        let mut r = ByteReader::new(&[1, 0, 0, 0, 0, 0, 0, 2]);
        assert_eq!(r.u32_le(), Ok(1));
        assert_eq!(r.u32_be(), Ok(2));
        assert!(r.is_at_end());
    }

    #[test]
    fn reading_past_the_end_is_an_error_and_does_not_move() {
        let mut r = ByteReader::new(&[1, 2, 3]);
        assert_eq!(
            r.u32_le(),
            Err(OutOfBounds {
                at: 0,
                wanted: 4,
                len: 3
            })
        );
        assert_eq!(r.pos(), 0);
        assert_eq!(r.bytes(usize::MAX).unwrap_err().wanted, usize::MAX);
    }
}
