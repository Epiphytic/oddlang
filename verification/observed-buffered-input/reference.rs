use std::io::{self, BufRead, Read};

const EARLY_EMPTY: bool = false;
const DISCARD_ON_ERROR: bool = false;
const ERASE_CONSUMED: bool = true;

struct Scanner<T> {
    source: T,
    block: zeroize::Zeroizing<[u8; 8]>,
    begin: usize,
    end: usize,
}

impl<T: Read> Read for Scanner<T> {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if EARLY_EMPTY && destination.is_empty() {
            return Ok(0);
        }
        let available = self.fill_buf()?;
        let length = available.len().min(destination.len());
        destination[..length].copy_from_slice(&available[..length]);
        self.consume(length);
        Ok(length)
    }
}

impl<T: Read> BufRead for Scanner<T> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.begin == self.end {
            self.begin = 0;
            match self.source.read(&mut self.block[..]) {
                Ok(length) => self.end = length,
                Err(error) => {
                    if DISCARD_ON_ERROR {
                        self.end = 0;
                    }
                    return Err(error);
                }
            }
        }
        Ok(&self.block[self.begin..self.end])
    }

    fn consume(&mut self, length: usize) {
        for _ in 0..length.min(self.end - self.begin) {
            if ERASE_CONSUMED {
                self.block[self.begin] = 0;
            }
            self.begin += 1;
        }
    }
}
