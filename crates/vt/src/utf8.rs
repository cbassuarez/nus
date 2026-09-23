//! Track UTF-8 byte boundaries without decoding or buffering the stream.
//!
//! This is not a replacement for vte's decoder. It only tells the caller when
//! an incoming byte belongs to an unfinished scalar, rather than a raw C1 code.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Utf8Prefix {
    remaining: u8,
    lower: u8,
    upper: u8,
}

impl Default for Utf8Prefix {
    fn default() -> Self {
        Self {
            remaining: 0,
            lower: 0x80,
            upper: 0xbf,
        }
    }
}

impl Utf8Prefix {
    #[inline]
    pub(crate) fn is_pending(self) -> bool {
        self.remaining != 0
    }

    /// Consume one byte; return true only for a valid continuation of the
    /// preceding prefix. Reject overlong encodings, surrogates, and > U+10FFFF.
    #[inline]
    pub(crate) fn observe(&mut self, byte: u8) -> bool {
        // Most terminal traffic is ASCII. Do not write state for each byte
        // when there is no UTF-8 prefix to complete.
        if self.remaining == 0 && byte < 0xc2 {
            return false;
        }
        if self.remaining != 0 && (self.lower..=self.upper).contains(&byte) {
            self.remaining -= 1;
            self.lower = 0x80;
            self.upper = 0xbf;
            return true;
        }

        *self = Self::default();
        match byte {
            0xc2..=0xdf => self.remaining = 1,
            0xe0 => {
                self.remaining = 2;
                self.lower = 0xa0;
            }
            0xed => {
                self.remaining = 2;
                self.upper = 0x9f;
            }
            0xe1..=0xec | 0xee..=0xef => self.remaining = 2,
            0xf0 => {
                self.remaining = 3;
                self.lower = 0x90;
            }
            0xf4 => {
                self.remaining = 3;
                self.upper = 0x8f;
            }
            0xf1..=0xf3 => self.remaining = 3,
            _ => {}
        }
        false
    }

    /// An unfinished UTF-8 scalar occupies at most the final three bytes.
    /// Inspect only that suffix, not the entire (potentially large) PTY read.
    pub(crate) fn at_end(bytes: &[u8]) -> Self {
        let mut prefix = Self::default();
        for &byte in &bytes[bytes.len().saturating_sub(3)..] {
            prefix.observe(byte);
        }
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8Prefix;

    #[test]
    fn c1_values_inside_utf8_are_continuations() {
        for text in ["面", "本", "한", "“", "”", "🙂", "🚀"] {
            let mut prefix = Utf8Prefix::default();
            for (index, byte) in text.bytes().enumerate() {
                assert_eq!(prefix.observe(byte), index != 0, "{text:?}: byte {index}");
            }
            assert!(!prefix.is_pending());
            assert!(!prefix.observe(0x9d), "a subsequent raw OSC is not Unicode");
        }
    }

    #[test]
    fn invalid_prefixes_do_not_hide_c1_controls() {
        for bytes in [
            b"\xe0\x9d".as_slice(),
            b"\xed\xa0",
            b"\xf0\x80",
            b"\xf4\x90",
            b"\xc0\x9d",
        ] {
            let mut prefix = Utf8Prefix::default();
            assert!(!prefix.observe(bytes[0]));
            assert!(!prefix.observe(bytes[1]), "invalid sequence: {bytes:?}");
            assert!(!prefix.is_pending());
        }
    }

    #[test]
    fn suffix_matches_incremental_state() {
        // The boundary values include all UTF-8 lead classes and their limits.
        let alphabet = [
            0x00, 0x1b, 0x20, 0x7f, 0x80, 0x8f, 0x90, 0x9c, 0x9d, 0x9f, 0xa0, 0xbf, 0xc0, 0xc2,
            0xdf, 0xe0, 0xe1, 0xed, 0xef, 0xf0, 0xf1, 0xf4, 0xf5, 0xff,
        ];
        for &a in &alphabet {
            for &b in &alphabet {
                for &c in &alphabet {
                    for &d in &alphabet {
                        let bytes = [a, b, c, d];
                        let mut prefix = Utf8Prefix::default();
                        for byte in bytes {
                            prefix.observe(byte);
                        }
                        assert_eq!(Utf8Prefix::at_end(&bytes), prefix, "{bytes:?}");
                    }
                }
            }
        }
        assert_eq!(Utf8Prefix::at_end(&[]), Utf8Prefix::default());
    }
}
