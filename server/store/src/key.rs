/// Builds keys whose byte order matches the order of their components.
///
/// Integers are big-endian; strings are length-prefixed so that a key built
/// from the first components is an exact prefix of the full key (the key of
/// guild "ab" never prefixes the keys of guild "abc").
#[derive(Debug, Clone, Default)]
pub struct Key(Vec<u8>);

impl Key {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u64(mut self, value: u64) -> Self {
        self.0.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn str(self, value: &str) -> Self {
        self.bytes(value.as_bytes())
    }

    pub fn bytes(mut self, value: &[u8]) -> Self {
        let len = u16::try_from(value.len()).expect("key components are shorter than 64 KiB");
        self.0.extend_from_slice(&len.to_be_bytes());
        self.0.extend_from_slice(value);
        self
    }

    /// Appends raw bytes with no length prefix. Only for fixed-width
    /// components such as hashes.
    pub fn raw(mut self, value: &[u8]) -> Self {
        self.0.extend_from_slice(value);
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.0
    }
}

/// Reads keys built with [`Key`], component by component.
pub struct KeyReader<'a>(&'a [u8]);

impl<'a> KeyReader<'a> {
    pub fn new(key: &'a [u8]) -> Self {
        Self(key)
    }

    pub fn u64(&mut self) -> Option<u64> {
        let (head, rest) = self.0.split_first_chunk::<8>()?;
        self.0 = rest;
        Some(u64::from_be_bytes(*head))
    }

    pub fn str(&mut self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes()?).ok()
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let (len, rest) = self.0.split_first_chunk::<2>()?;
        let len = u16::from_be_bytes(*len) as usize;
        if rest.len() < len {
            return None;
        }
        let (value, rest) = rest.split_at(len);
        self.0 = rest;
        Some(value)
    }

    pub fn raw(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.0.len() < len {
            return None;
        }
        let (value, rest) = self.0.split_at(len);
        self.0 = rest;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_round_trip_and_sort() {
        let key = Key::new().str("guild").u64(42).build();
        let mut reader = KeyReader::new(&key);
        assert_eq!(reader.str(), Some("guild"));
        assert_eq!(reader.u64(), Some(42));

        let short = Key::new().str("ab").build();
        let long = Key::new().str("abc").u64(0).build();
        assert!(!long.starts_with(&short));

        let a = Key::new().str("g").u64(9).build();
        let b = Key::new().str("g").u64(10).build();
        assert!(a < b);
    }
}
