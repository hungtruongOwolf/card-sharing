//! Minimal CBOR decoder (RFC 8949) for App Attest objects: unsigned/negative integers, byte and
//! text strings, arrays, maps, simple values. Definite lengths only, bounded depth and size.

use crate::AttestationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Uint(u64),
    Negative(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Simple(u8),
}

impl Value {
    /// The value under a text key of a map.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Map(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Self::Text(t) if t == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

const MAX_DEPTH: usize = 8;
const MAX_ITEMS: u64 = 1024;
const MALFORMED: AttestationError = AttestationError::Malformed("CBOR");

/// Decodes exactly one item; trailing bytes are an error.
pub(crate) fn decode(input: &[u8]) -> Result<Value, AttestationError> {
    let mut pos = 0;
    let value = item(input, &mut pos, 0)?;
    if pos != input.len() {
        return Err(MALFORMED);
    }
    Ok(value)
}

fn item(input: &[u8], pos: &mut usize, depth: usize) -> Result<Value, AttestationError> {
    if depth > MAX_DEPTH {
        return Err(MALFORMED);
    }
    let initial = *input.get(*pos).ok_or(MALFORMED)?;
    *pos += 1;
    let major = initial >> 5;
    let info = initial & 0x1f;
    let arg = argument(input, pos, info)?;
    match major {
        0 => Ok(Value::Uint(arg)),
        1 => Ok(Value::Negative(arg)),
        2 => Ok(Value::Bytes(take(input, pos, arg)?.to_vec())),
        3 => String::from_utf8(take(input, pos, arg)?.to_vec())
            .map(Value::Text)
            .map_err(|_| MALFORMED),
        4 => {
            if arg > MAX_ITEMS {
                return Err(MALFORMED);
            }
            (0..arg)
                .map(|_| item(input, pos, depth + 1))
                .collect::<Result<_, _>>()
                .map(Value::Array)
        }
        5 => {
            if arg > MAX_ITEMS {
                return Err(MALFORMED);
            }
            (0..arg)
                .map(|_| Ok((item(input, pos, depth + 1)?, item(input, pos, depth + 1)?)))
                .collect::<Result<_, _>>()
                .map(Value::Map)
        }
        7 if info < 24 => Ok(Value::Simple(info)),
        _ => Err(MALFORMED), // tags, floats, indefinite lengths
    }
}

fn argument(input: &[u8], pos: &mut usize, info: u8) -> Result<u64, AttestationError> {
    let width = match info {
        0..=23 => return Ok(u64::from(info)),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err(MALFORMED),
    };
    let bytes = take(input, pos, width)?;
    Ok(bytes.iter().fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
}

fn take<'a>(input: &'a [u8], pos: &mut usize, len: u64) -> Result<&'a [u8], AttestationError> {
    let len = usize::try_from(len).map_err(|_| MALFORMED)?;
    let end = pos.checked_add(len).ok_or(MALFORMED)?;
    let slice = input.get(*pos..end).ok_or(MALFORMED)?;
    *pos = end;
    Ok(slice)
}

/// Encoder for test vectors.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn encode(value: &Value) -> Vec<u8> {
    fn head(out: &mut Vec<u8>, major: u8, arg: u64) {
        let major = major << 5;
        match arg {
            0..=23 => out.push(major | arg as u8),
            24..=0xff => out.extend([major | 24, arg as u8]),
            0x100..=0xffff => {
                out.push(major | 25);
                out.extend((arg as u16).to_be_bytes());
            }
            0x1_0000..=0xffff_ffff => {
                out.push(major | 26);
                out.extend((arg as u32).to_be_bytes());
            }
            _ => {
                out.push(major | 27);
                out.extend(arg.to_be_bytes());
            }
        }
    }
    fn go(out: &mut Vec<u8>, value: &Value) {
        match value {
            Value::Uint(n) => head(out, 0, *n),
            Value::Negative(n) => head(out, 1, *n),
            Value::Bytes(b) => {
                head(out, 2, b.len() as u64);
                out.extend(b);
            }
            Value::Text(t) => {
                head(out, 3, t.len() as u64);
                out.extend(t.as_bytes());
            }
            Value::Array(items) => {
                head(out, 4, items.len() as u64);
                items.iter().for_each(|i| go(out, i));
            }
            Value::Map(entries) => {
                head(out, 5, entries.len() as u64);
                for (k, v) in entries {
                    go(out, k);
                    go(out, v);
                }
            }
            Value::Simple(s) => head(out, 7, u64::from(*s)),
        }
    }
    let mut out = Vec::new();
    go(&mut out, value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_nested_values() {
        let value = Value::Map(vec![
            (
                Value::Text("fmt".into()),
                Value::Text("apple-appattest".into()),
            ),
            (
                Value::Text("attStmt".into()),
                Value::Map(vec![(
                    Value::Text("x5c".into()),
                    Value::Array(vec![Value::Bytes(vec![1; 300]), Value::Bytes(vec![])]),
                )]),
            ),
            (Value::Text("n".into()), Value::Uint(70_000)),
        ]);
        let decoded = decode(&encode(&value)).unwrap();
        assert_eq!(decoded, value);
        assert_eq!(
            decoded.get("fmt").and_then(Value::as_text),
            Some("apple-appattest")
        );
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(decode(&[]).is_err());
        assert!(decode(&[0x5f]).is_err(), "indefinite length");
        assert!(decode(&[0x42, 0x01]).is_err(), "truncated");
        assert!(decode(&[0x01, 0x01]).is_err(), "trailing bytes");
        assert!(decode(&[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).is_err());
        // Deep nesting is bounded.
        assert!(decode(&[0x81; 20].iter().copied().chain([0x01]).collect::<Vec<_>>()).is_err());
    }
}
