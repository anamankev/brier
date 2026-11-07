//! CRC-32C (Castagnoli), as used in the journal record format (ADR-0006).
//!
//! Reflected polynomial `0x82F63B78`, initial value and final XOR `0xFFFF_FFFF`, the variant
//! used by iSCSI, ext4, and most log-structured stores. Table-driven, one byte at a time: the
//! journal checksums small records, so this is far from the bottleneck (the sync is).

const POLY: u32 = 0x82F6_3B78;

// Loop counters run 0..256 and 0..8, so they cannot overflow.
#[allow(clippy::arithmetic_side_effects)]
const fn table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut crc = i;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        #[allow(clippy::indexing_slicing, clippy::cast_possible_truncation)] // i < 256
        {
            table[i as usize] = crc;
        }
        i += 1;
    }
    table
}

static TABLE: [u32; 256] = table();

/// An incremental CRC-32C.
#[derive(Debug, Clone, Copy)]
pub struct Crc32c(u32);

impl Default for Crc32c {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32c {
    /// A checksum over no bytes yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    /// Add `bytes` to the checksum.
    #[must_use]
    pub fn update(self, bytes: &[u8]) -> Self {
        let mut crc = self.0;
        for &b in bytes {
            // The index is a u8 widened to usize, so it is always below 256; the cast keeps
            // the low byte of the running CRC, which is the point.
            #[allow(clippy::indexing_slicing, clippy::cast_possible_truncation)]
            let entry = TABLE[usize::from((crc as u8) ^ b)];
            crc = (crc >> 8) ^ entry;
        }
        Self(crc)
    }

    /// The final checksum value.
    #[must_use]
    pub const fn finish(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

/// CRC-32C of `bytes`.
#[must_use]
pub fn crc32c(bytes: &[u8]) -> u32 {
    Crc32c::new().update(bytes).finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_check_value() {
        // The CRC catalogue's check value for CRC-32/ISCSI (CRC-32C): "123456789" -> 0xE3069283.
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }

    #[test]
    fn incremental_updates_equal_one_shot() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let (a, b) = data.split_at(17);
        assert_eq!(Crc32c::new().update(a).update(b).finish(), crc32c(data));
    }

    #[test]
    fn every_single_bit_flip_changes_the_checksum() {
        let data = *b"journal record payload";
        let good = crc32c(&data);
        for i in 0..data.len() {
            for bit in 0..8 {
                let mut bad = data;
                bad[i] ^= 1 << bit;
                assert_ne!(crc32c(&bad), good, "byte {i} bit {bit}");
            }
        }
    }
}
