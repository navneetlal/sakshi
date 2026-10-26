//! A reader for the metadata header of GGUF files.
//!
//! Only the key/value metadata is parsed; tensor data is never touched. The
//! parser works on a byte prefix of the file and reports
//! [`GgufError::Truncated`] when it needs more, so callers can read a remote
//! file's header with HTTP range requests.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAGIC: &[u8; 4] = b"GGUF";
/// Upper bound on metadata entries; real files have well under a hundred.
const MAX_KV: u64 = 65_536;

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("not a GGUF file")]
    NotGguf,
    #[error("unsupported GGUF version {0}")]
    UnsupportedVersion(u32),
    #[error("GGUF header needs more than the {0} bytes read")]
    Truncated(usize),
    #[error("malformed GGUF header: {0}")]
    Malformed(String),
    #[error("reading GGUF header: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    Uint(u64),
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Array(Vec<GgufValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct GgufMetadata {
    pub version: u32,
    pub tensor_count: u64,
    pub kv: BTreeMap<String, GgufValue>,
}

impl GgufMetadata {
    /// Parses the header from the start of a GGUF file.
    pub fn parse(bytes: &[u8]) -> Result<Self, GgufError> {
        let mut r = Cursor { bytes, pos: 0 };
        if bytes.len() >= 4 && &bytes[..4] != MAGIC {
            return Err(GgufError::NotGguf);
        }
        r.take(4)?;
        let version = r.u32()?;
        if !(2..=3).contains(&version) {
            return Err(GgufError::UnsupportedVersion(version));
        }
        let tensor_count = r.u64()?;
        let kv_count = r.u64()?;
        if kv_count > MAX_KV {
            return Err(GgufError::Malformed(format!("{kv_count} metadata entries")));
        }
        let mut kv = BTreeMap::new();
        for _ in 0..kv_count {
            let key = r.string()?;
            let ty = r.u32()?;
            let value = r.value(ty)?;
            kv.insert(key, value);
        }
        Ok(Self {
            version,
            tensor_count,
            kv,
        })
    }

    /// Reads a local file's header, growing the read until the header fits.
    pub fn read_file(path: &Path) -> Result<Self, GgufError> {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        let mut want = 1 << 20;
        loop {
            let have = buf.len();
            (&mut file)
                .take((want - have) as u64)
                .read_to_end(&mut buf)?;
            let eof = buf.len() < want;
            match Self::parse(&buf) {
                Err(GgufError::Truncated(_)) if !eof => want *= 4,
                Err(GgufError::Truncated(n)) => {
                    return Err(GgufError::Malformed(format!(
                        "file ends inside the header after {n} bytes"
                    )));
                }
                other => return other,
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&GgufValue> {
        self.kv.get(key)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            GgufValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// `general.architecture`, e.g. `asr` or `sortformer`.
    pub fn architecture(&self) -> Option<&str> {
        self.get_str("general.architecture")
    }

    /// A string array, or `None` if the key is missing or holds anything else.
    pub fn string_array(&self, key: &str) -> Option<Vec<&str>> {
        match self.get(key)? {
            GgufValue::Array(items) => items
                .iter()
                .map(|v| match v {
                    GgufValue::String(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], GgufError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(GgufError::Truncated(self.bytes.len()))?;
        let out = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], GgufError> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    fn u32(&mut self) -> Result<u32, GgufError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, GgufError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn len(&mut self) -> Result<usize, GgufError> {
        let n = self.u64()?;
        // A length larger than the remaining input can only be satisfied by
        // reading more, so it is reported as truncation, never allocated.
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= self.bytes.len() - self.pos)
            .ok_or(GgufError::Truncated(self.bytes.len()))
    }

    fn string(&mut self) -> Result<String, GgufError> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| GgufError::Malformed("string is not UTF-8".into()))
    }

    fn value(&mut self, ty: u32) -> Result<GgufValue, GgufError> {
        Ok(match ty {
            0 => GgufValue::Uint(self.array::<1>()?[0].into()),
            1 => GgufValue::Int(i8::from_le_bytes(self.array()?).into()),
            2 => GgufValue::Uint(u16::from_le_bytes(self.array()?).into()),
            3 => GgufValue::Int(i16::from_le_bytes(self.array()?).into()),
            4 => GgufValue::Uint(self.u32()?.into()),
            5 => GgufValue::Int(i32::from_le_bytes(self.array()?).into()),
            6 => GgufValue::Float(f32::from_le_bytes(self.array()?).into()),
            7 => GgufValue::Bool(self.array::<1>()?[0] != 0),
            8 => GgufValue::String(self.string()?),
            9 => {
                let elem_ty = self.u32()?;
                if elem_ty == 9 {
                    return Err(GgufError::Malformed("nested arrays".into()));
                }
                // Every element takes at least one byte, so `len` bounds the
                // count by the remaining input before anything is allocated.
                let count = self.len()?;
                let mut items = Vec::with_capacity(count.min(4096));
                for _ in 0..count {
                    items.push(self.value(elem_ty)?);
                }
                GgufValue::Array(items)
            }
            10 => GgufValue::Uint(self.u64()?),
            11 => GgufValue::Int(i64::from_le_bytes(self.array()?)),
            12 => GgufValue::Float(f64::from_le_bytes(self.array()?)),
            other => return Err(GgufError::Malformed(format!("unknown value type {other}"))),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a GGUF v3 header with the given string and string-array keys.
    pub(crate) fn header(strings: &[(&str, &str)], arrays: &[(&str, &[&str])]) -> Vec<u8> {
        fn put_str(out: &mut Vec<u8>, s: &str) {
            out.extend((s.len() as u64).to_le_bytes());
            out.extend(s.as_bytes());
        }
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend(3u32.to_le_bytes());
        out.extend(42u64.to_le_bytes());
        out.extend(((strings.len() + arrays.len() + 1) as u64).to_le_bytes());
        for (key, value) in strings {
            put_str(&mut out, key);
            out.extend(8u32.to_le_bytes());
            put_str(&mut out, value);
        }
        for (key, values) in arrays {
            put_str(&mut out, key);
            out.extend(9u32.to_le_bytes());
            out.extend(8u32.to_le_bytes());
            out.extend((values.len() as u64).to_le_bytes());
            for v in *values {
                put_str(&mut out, v);
            }
        }
        put_str(&mut out, "asr.encoder.pos_emb_max_len");
        out.extend(4u32.to_le_bytes());
        out.extend(5000u32.to_le_bytes());
        out.extend([0u8; 64]); // stand-in for tensor info that follows
        out
    }

    #[test]
    fn parses_metadata() {
        let bytes = header(
            &[("general.architecture", "asr")],
            &[(
                "asr.rnnt.prompt_dictionary",
                &["en-US:0", "hi:6", "auto:101"],
            )],
        );

        let meta = GgufMetadata::parse(&bytes).unwrap();

        assert_eq!(meta.version, 3);
        assert_eq!(meta.tensor_count, 42);
        assert_eq!(meta.architecture(), Some("asr"));
        assert_eq!(
            meta.string_array("asr.rnnt.prompt_dictionary").unwrap(),
            ["en-US:0", "hi:6", "auto:101"]
        );
        assert_eq!(
            meta.get("asr.encoder.pos_emb_max_len"),
            Some(&GgufValue::Uint(5000))
        );
    }

    #[test]
    fn reports_truncation_at_every_cut() {
        let bytes = header(&[("general.architecture", "sortformer")], &[]);
        let header_len = bytes.len() - 64;
        for cut in 4..header_len {
            assert!(
                matches!(
                    GgufMetadata::parse(&bytes[..cut]),
                    Err(GgufError::Truncated(_))
                ),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn rejects_other_formats() {
        assert!(matches!(
            GgufMetadata::parse(b"lmgg\x01\x00\x00\x00"),
            Err(GgufError::NotGguf)
        ));
        assert!(matches!(
            GgufMetadata::parse(b"PK\x03\x04 a zip, not a model"),
            Err(GgufError::NotGguf)
        ));
    }

    #[test]
    fn huge_lengths_do_not_allocate() {
        let mut bytes = header(&[], &[]);
        // Corrupt the first key length to u64::MAX.
        bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            GgufMetadata::parse(&bytes),
            Err(GgufError::Truncated(_))
        ));
    }

    #[test]
    fn reads_header_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.gguf");
        std::fs::write(&path, header(&[("general.architecture", "asr")], &[])).unwrap();
        assert_eq!(
            GgufMetadata::read_file(&path).unwrap().architecture(),
            Some("asr")
        );

        std::fs::write(
            &path,
            &header(&[("general.architecture", "asr")], &[])[..30],
        )
        .unwrap();
        assert!(matches!(
            GgufMetadata::read_file(&path),
            Err(GgufError::Malformed(_))
        ));
    }
}
