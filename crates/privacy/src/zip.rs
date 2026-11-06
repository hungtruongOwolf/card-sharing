//! A minimal ZIP writer for data exports (T-154): stored (uncompressed) entries with CRC-32,
//! UTF-8 names, no ZIP64. The images inside are already compressed, so storing costs little and
//! needs no compression dependency. Any reader (Finder, Explorer, unzip) opens the result.

/// Largest archive or entry this writer produces (no ZIP64).
pub const MAX_ZIP_BYTES: usize = u32::MAX as usize;

#[derive(Debug, thiserror::Error)]
pub enum ZipError {
    #[error("the archive would exceed 4 GiB")]
    TooLarge,
    #[error("too many entries")]
    TooManyEntries,
    #[error("invalid entry name {0:?}")]
    InvalidName(String),
}

/// Builds an archive in memory.
#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

/// DOS date of 1980-01-01 (time 00:00): fixed, so archives of the same data are identical.
const DOS_DATE: u16 = (1 << 5) | 1;
/// Bit 11: names are UTF-8.
const UTF8_FLAG: u16 = 1 << 11;

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn checked_u32(value: usize) -> Result<u32, ZipError> {
    u32::try_from(value).map_err(|_| ZipError::TooLarge)
}

impl ZipWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one file. Names use `/` separators and may not be absolute or contain `..`.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), ZipError> {
        if name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|part| part == ".." || part.is_empty())
            || name.len() > usize::from(u16::MAX)
        {
            return Err(ZipError::InvalidName(name.to_owned()));
        }
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or(ZipError::TooManyEntries)?;
        let crc = crc32fast::hash(data);
        let size = checked_u32(data.len())?;
        let offset = checked_u32(self.out.len())?;
        let name_len =
            u16::try_from(name.len()).map_err(|_| ZipError::InvalidName(name.to_owned()))?;

        let out = &mut self.out;
        u32le(out, 0x0403_4b50);
        u16le(out, 20); // version needed
        u16le(out, UTF8_FLAG);
        u16le(out, 0); // stored
        u16le(out, 0); // time
        u16le(out, DOS_DATE);
        u32le(out, crc);
        u32le(out, size);
        u32le(out, size);
        u16le(out, name_len);
        u16le(out, 0); // extra
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        if self.out.len() > MAX_ZIP_BYTES {
            return Err(ZipError::TooLarge);
        }

        let c = &mut self.central;
        u32le(c, 0x0201_4b50);
        u16le(c, 20); // version made by
        u16le(c, 20); // version needed
        u16le(c, UTF8_FLAG);
        u16le(c, 0);
        u16le(c, 0);
        u16le(c, DOS_DATE);
        u32le(c, crc);
        u32le(c, size);
        u32le(c, size);
        u16le(c, name_len);
        u16le(c, 0); // extra
        u16le(c, 0); // comment
        u16le(c, 0); // disk
        u16le(c, 0); // internal attributes
        u32le(c, 0); // external attributes
        u32le(c, offset);
        c.extend_from_slice(name.as_bytes());
        Ok(())
    }

    /// The finished archive.
    pub fn finish(mut self) -> Result<Vec<u8>, ZipError> {
        let central_offset = checked_u32(self.out.len())?;
        let central_size = checked_u32(self.central.len())?;
        self.out.extend_from_slice(&self.central);
        let out = &mut self.out;
        u32le(out, 0x0605_4b50);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, self.entries);
        u16le(out, self.entries);
        u32le(out, central_size);
        u32le(out, central_offset);
        u16le(out, 0); // comment
        if out.len() > MAX_ZIP_BYTES {
            return Err(ZipError::TooLarge);
        }
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn writes_a_readable_stored_archive() {
        let mut zip = ZipWriter::new();
        zip.add("account.json", b"{\"a\":1}").unwrap();
        zip.add("media/card_render/x.png", &[1, 2, 3]).unwrap();
        let bytes = zip.finish().unwrap();
        assert_eq!(u32_at(&bytes, 0), 0x0403_4b50);
        // End of central directory: two entries, pointing at the central directory.
        let eocd = bytes.len() - 22;
        assert_eq!(u32_at(&bytes, eocd), 0x0605_4b50);
        assert_eq!(u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]), 2);
        let central = u32_at(&bytes, eocd + 16) as usize;
        assert_eq!(u32_at(&bytes, central), 0x0201_4b50);
        // CRC of the first entry.
        assert_eq!(u32_at(&bytes, 14), crc32fast::hash(b"{\"a\":1}"));
    }

    #[test]
    fn refuses_unsafe_names() {
        let mut zip = ZipWriter::new();
        for name in ["", "/etc/passwd", "../x", "a//b", "a\\b"] {
            assert!(zip.add(name, b"x").is_err(), "{name}");
        }
    }
}
