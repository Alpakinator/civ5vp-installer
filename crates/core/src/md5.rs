//! MD5 (RFC 1321), for the one place the Core needs it: the `md5="…"` of a `<File>` line that
//! Modinfo Completion adds, written the way ModBuddy and upstream's `generate_modinfo.py` write
//! it - uppercase hex of the file's bytes.
//!
//! Not a security measure, and the game does not check it on load (ADR-0014). Here, rather than
//! behind a boundary, because the Core has no dependencies and this is a hundred lines.

/// The MD5 of `bytes` as ModBuddy writes it into a `.modinfo`: 32 uppercase hex digits.
pub(crate) fn md5_hex_upper(bytes: &[u8]) -> String {
    digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

/// Per-round left-rotation amounts.
const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor(abs(sin(i + 1)) * 2^32)`, as RFC 1321 tabulates it.
const SINES: [u32; 64] = [
    0xd76a_a478,
    0xe8c7_b756,
    0x2420_70db,
    0xc1bd_ceee,
    0xf57c_0faf,
    0x4787_c62a,
    0xa830_4613,
    0xfd46_9501,
    0x6980_98d8,
    0x8b44_f7af,
    0xffff_5bb1,
    0x895c_d7be,
    0x6b90_1122,
    0xfd98_7193,
    0xa679_438e,
    0x49b4_0821,
    0xf61e_2562,
    0xc040_b340,
    0x265e_5a51,
    0xe9b6_c7aa,
    0xd62f_105d,
    0x0244_1453,
    0xd8a1_e681,
    0xe7d3_fbc8,
    0x21e1_cde6,
    0xc337_07d6,
    0xf4d5_0d87,
    0x455a_14ed,
    0xa9e3_e905,
    0xfcef_a3f8,
    0x676f_02d9,
    0x8d2a_4c8a,
    0xfffa_3942,
    0x8771_f681,
    0x6d9d_6122,
    0xfde5_380c,
    0xa4be_ea44,
    0x4bde_cfa9,
    0xf6bb_4b60,
    0xbebf_bc70,
    0x289b_7ec6,
    0xeaa1_27fa,
    0xd4ef_3085,
    0x0488_1d05,
    0xd9d4_d039,
    0xe6db_99e5,
    0x1fa2_7cf8,
    0xc4ac_5665,
    0xf429_2244,
    0x432a_ff97,
    0xab94_23a7,
    0xfc93_a039,
    0x655b_59c3,
    0x8f0c_cc92,
    0xffef_f47d,
    0x8584_5dd1,
    0x6fa8_7e4f,
    0xfe2c_e6e0,
    0xa301_4314,
    0x4e08_11a1,
    0xf753_7e82,
    0xbd3a_f235,
    0x2ad7_d2bb,
    0xeb86_d391,
];

fn digest(bytes: &[u8]) -> [u8; 16] {
    let mut state: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

    // Padding: a 1 bit, zeros to 56 mod 64 bytes, then the bit length, little-endian.
    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    let mut tail = bytes[bytes.len() - bytes.len() % 64..].to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_length.to_le_bytes());

    let whole_blocks = &bytes[..bytes.len() - bytes.len() % 64];
    for block in whole_blocks.chunks_exact(64).chain(tail.chunks_exact(64)) {
        compress(&mut state, block);
    }

    let mut out = [0u8; 16];
    for (word, chunk) in state.iter().zip(out.chunks_exact_mut(4)) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

fn compress(state: &mut [u32; 4], block: &[u8]) {
    let mut words = [0u32; 16];
    for (word, chunk) in words.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for round in 0..64 {
        let (mixed, index) = match round / 16 {
            0 => ((b & c) | (!b & d), round),
            1 => ((d & b) | (!d & c), (5 * round + 1) % 16),
            2 => (b ^ c ^ d, (3 * round + 5) % 16),
            _ => (c ^ (b | !d), (7 * round) % 16),
        };
        let rotated = a
            .wrapping_add(mixed)
            .wrapping_add(SINES[round])
            .wrapping_add(words[index])
            .rotate_left(SHIFTS[round]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(rotated);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

#[cfg(test)]
mod tests {
    use super::md5_hex_upper;

    /// The test suite of RFC 1321, appendix A.5.
    #[test]
    fn the_rfc_1321_test_suite() {
        let suite = [
            ("", "D41D8CD98F00B204E9800998ECF8427E"),
            ("a", "0CC175B9C0F1B6A831C399E269772661"),
            ("abc", "900150983CD24FB0D6963F7D28E17F72"),
            ("message digest", "F96B697D7CB7938D525A2F31AAF161D0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "C3FCD3D76192E4007DFB496CCA67E13B",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "D174AB98D277D9F5A5611C2C9F419D9F",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57EDF4A22BE3C955AC49DA2E2107B67A",
            ),
        ];
        for (input, expected) in suite {
            assert_eq!(
                md5_hex_upper(input.as_bytes()),
                expected,
                "MD5 of {input:?}"
            );
        }
    }

    /// Lengths around the 56- and 64-byte padding edges, against values from `md5sum`.
    #[test]
    fn the_padding_edges() {
        let cases = [
            (55, "EF1772B6DFF9A122358552954AD0DF65"),
            (56, "3B0C8AC703F828B04C6C197006D17218"),
            (63, "B06521F39153D618550606BE297466D5"),
            (64, "014842D480B571495A4A0363793F7367"),
            (65, "C743A45E0D2E6A95CB859ADAE0248435"),
        ];
        for (length, expected) in cases {
            assert_eq!(
                md5_hex_upper(&vec![b'a'; length]),
                expected,
                "{length} bytes"
            );
        }
    }
}
