//! A record body that refuses to exceed its per-record cap. Oversize
//! payloads surface as an I/O error at serialization time, so admission
//! can mark the capture incomplete instead of writing a partial line.

use std::io::{self, Write};

pub(crate) struct Bytes {
    bytes: Vec<u8>,
    limit: usize,
}

impl Bytes {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("record limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversize_write_fails_instead_of_truncating() {
        let mut bytes = Bytes::new(8);
        assert_eq!(bytes.write(b"abc").unwrap(), 3);
        assert!(bytes.write(b"defghij").is_err());
        assert_eq!(bytes.into_vec(), b"abc".to_vec());
    }
}
