//! Minimal DER reader: just enough X.509 to read a leaf certificate's public key and extensions.
//! Chain validation is done by `rustls-webpki`; this never decides trust on its own.

use crate::AttestationError;

pub(crate) const TAG_SEQUENCE: u8 = 0x30;
pub(crate) const TAG_OID: u8 = 0x06;
pub(crate) const TAG_BOOLEAN: u8 = 0x01;
pub(crate) const TAG_OCTET_STRING: u8 = 0x04;
pub(crate) const TAG_BIT_STRING: u8 = 0x03;

const MALFORMED: AttestationError = AttestationError::Malformed("certificate DER");

pub(crate) struct Der<'a> {
    data: &'a [u8],
}

impl<'a> Der<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub(crate) fn peek_tag(&self) -> Option<u8> {
        self.data.first().copied()
    }

    /// Reads one TLV; returns `(tag, contents)`.
    pub(crate) fn read(&mut self) -> Result<(u8, &'a [u8]), AttestationError> {
        let (&tag, rest) = self.data.split_first().ok_or(MALFORMED)?;
        let (&first, mut rest) = rest.split_first().ok_or(MALFORMED)?;
        let len = if first < 0x80 {
            usize::from(first)
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 || count > 4 || rest.len() < count {
                return Err(MALFORMED);
            }
            let (len_bytes, after) = rest.split_at(count);
            rest = after;
            len_bytes
                .iter()
                .fold(0usize, |acc, b| (acc << 8) | usize::from(*b))
        };
        if rest.len() < len {
            return Err(MALFORMED);
        }
        let (contents, after) = rest.split_at(len);
        self.data = after;
        Ok((tag, contents))
    }

    pub(crate) fn expect(&mut self, tag: u8) -> Result<&'a [u8], AttestationError> {
        let (found, contents) = self.read()?;
        if found == tag {
            Ok(contents)
        } else {
            Err(MALFORMED)
        }
    }
}

/// The public key bits and the extensions `(oid, value)` of a certificate.
pub(crate) struct CertParts<'a> {
    pub(crate) public_key: &'a [u8],
    pub(crate) extensions: Vec<(&'a [u8], &'a [u8])>,
}

pub(crate) fn cert_parts(cert: &[u8]) -> Result<CertParts<'_>, AttestationError> {
    let mut outer = Der::new(Der::new(cert).expect(TAG_SEQUENCE)?);
    let mut tbs = Der::new(outer.expect(TAG_SEQUENCE)?);
    if tbs.peek_tag() == Some(0xa0) {
        tbs.read()?; // version
    }
    for _ in 0..5 {
        tbs.read()?; // serial, signature algorithm, issuer, validity, subject
    }
    let mut spki = Der::new(tbs.expect(TAG_SEQUENCE)?);
    spki.expect(TAG_SEQUENCE)?; // algorithm
    let bits = spki.expect(TAG_BIT_STRING)?;
    let (&unused, public_key) = bits.split_first().ok_or(MALFORMED)?;
    if unused != 0 {
        return Err(MALFORMED);
    }
    let mut extensions = Vec::new();
    while !tbs.is_empty() {
        let (tag, contents) = tbs.read()?;
        if tag != 0xa3 {
            continue; // issuer/subject unique ids
        }
        let mut list = Der::new(Der::new(contents).expect(TAG_SEQUENCE)?);
        while !list.is_empty() {
            let mut ext = Der::new(list.expect(TAG_SEQUENCE)?);
            let oid = ext.expect(TAG_OID)?;
            if ext.peek_tag() == Some(TAG_BOOLEAN) {
                ext.read()?;
            }
            extensions.push((oid, ext.expect(TAG_OCTET_STRING)?));
        }
    }
    Ok(CertParts {
        public_key,
        extensions,
    })
}

/// The uncompressed P-256 point of a DER `SubjectPublicKeyInfo`.
pub(crate) fn spki_point(spki: &[u8]) -> Result<&[u8], AttestationError> {
    let mut seq = Der::new(Der::new(spki).expect(TAG_SEQUENCE)?);
    seq.expect(TAG_SEQUENCE)?;
    let bits = seq.expect(TAG_BIT_STRING)?;
    match bits.split_first() {
        Some((0, point)) if point.len() == 65 && point[0] == 0x04 => Ok(point),
        _ => Err(AttestationError::Malformed("public key")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_short_and_long_lengths() {
        let mut short = Der::new(&[0x04, 0x02, 0xaa, 0xbb]);
        assert_eq!(short.read().unwrap(), (0x04, &[0xaa, 0xbb][..]));
        let mut long_form = vec![0x04, 0x81, 0x80];
        long_form.extend(std::iter::repeat_n(7u8, 128));
        let mut long = Der::new(&long_form);
        assert_eq!(long.read().unwrap().1.len(), 128);
    }

    #[test]
    fn truncated_input_is_rejected() {
        assert!(Der::new(&[0x04, 0x05, 0x01]).read().is_err());
        assert!(Der::new(&[0x04, 0x85, 1, 1, 1, 1, 1]).read().is_err());
        assert!(Der::new(&[]).read().is_err());
    }
}
