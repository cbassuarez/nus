//! Bounded output retention. This exact executable source is checked by Verus.
//! Normal Cargo builds erase only the cfg-gated specifications and proof hints.
#[cfg(not(verus_keep_ghost))]
use verus_builtin_macros::{verus_spec, verus_verify};
#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg(verus_keep_ghost)]
verus! {
    impl Ring {
        pub closed spec fn contents(&self) -> Seq<u8> { self.buf@ }
        pub closed spec fn capacity(&self) -> usize { self.cap }
    }
    pub open spec fn suffix(bytes: Seq<u8>, cap: usize) -> Seq<u8> {
        if bytes.len() <= cap { bytes }
        else { bytes.subrange(bytes.len() - cap, bytes.len() as int) }
    }
}

#[verus_verify]
pub struct Ring {
    buf: Vec<u8>,
    cap: usize,
}

#[verus_verify]
impl Ring {
    #[verus_spec(result =>
        ensures result.contents() == Seq::<u8>::empty(), result.capacity() == cap,
    )]
    pub fn new(cap: usize) -> Ring {
        Ring {
            buf: Vec::with_capacity(cap.min(1 << 16)),
            cap,
        }
    }

    #[verus_spec(
        requires old(self).contents().len() <= old(self).capacity(),
        ensures
            final(self).capacity() == old(self).capacity(),
            final(self).contents().len() <= final(self).capacity(),
            final(self).contents() == suffix(old(self).contents() + bytes@, old(self).capacity()),
    )]
    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= self.cap {
            self.buf.clear();
            self.buf.extend_from_slice(&bytes[bytes.len() - self.cap..]);
        } else {
            // Subtract before adding: len + input can overflow even when the
            // retained result fits. Bulk copy keeps the existing fast path.
            let room = self.cap - bytes.len();
            if self.buf.len() > room {
                let cut = self.buf.len() - room;
                self.buf.copy_within(cut.., 0);
                self.buf.truncate(room);
            }
            self.buf.extend_from_slice(bytes);
        }
        #[cfg(verus_keep_ghost)]
        proof! {
            assert(self.buf@ =~= suffix(old(self).contents() + bytes@, old(self).capacity()));
        }
    }

    #[verus_spec(result => ensures result@ == self.contents())]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    #[cfg(all(test, not(verus_keep_ghost)))]
    pub(crate) fn allocated_capacity(&self) -> usize {
        self.buf.capacity()
    }
}

#[cfg(all(test, not(verus_keep_ghost)))]
mod tests {
    use super::Ring;

    #[test]
    fn retention_matches_an_unbounded_history_oracle() {
        for cap in [0, 1, 2, 7, 64, 255, 1024] {
            let mut ring = Ring::new(cap);
            let mut history = Vec::new();
            let mut seed = 0x1234_5678u32;
            for turn in 0..500 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let len = if turn % 11 == 0 {
                    0
                } else {
                    seed as usize % 1500
                };
                let input: Vec<u8> = (0..len).map(|i| (i ^ turn) as u8).collect();
                history.extend_from_slice(&input);
                ring.push(&input);
                assert_eq!(ring.bytes(), &history[history.len().saturating_sub(cap)..]);
            }
        }
    }
}
