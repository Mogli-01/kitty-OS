//! ChaCha20 (RFC 8439) — educational, not audited.
//!
//! Only additions, rotations and XORs on 32-bit words: no secret-dependent
//! branches or memory accesses, so it runs in constant time.

/// Key size in bytes.
pub const KEY_LEN: usize = 32;
/// Nonce size in bytes (RFC 8439: 96 bits).
pub const NONCE_LEN: usize = 12;
/// Output of one block.
pub const BLOCK_LEN: usize = 64;

#[inline(always)]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// The ChaCha20 block function: 64 bytes of keystream for
/// (`key`, `counter`, `nonce`) (RFC 8439 §2.3).
pub fn block(key: &[u8; KEY_LEN], counter: u32, nonce: &[u8; NONCE_LEN]) -> [u8; BLOCK_LEN] {
    let word = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut init = [0u32; 16];
    init[0] = 0x6170_7865;
    init[1] = 0x3320_646e;
    init[2] = 0x7962_2d32;
    init[3] = 0x6b20_6574;
    for i in 0..8 {
        init[4 + i] = word(&key[4 * i..4 * i + 4]);
    }
    init[12] = counter;
    for i in 0..3 {
        init[13 + i] = word(&nonce[4 * i..4 * i + 4]);
    }
    let mut s = init;
    for _ in 0..10 {
        quarter_round(&mut s, 0, 4, 8, 12);
        quarter_round(&mut s, 1, 5, 9, 13);
        quarter_round(&mut s, 2, 6, 10, 14);
        quarter_round(&mut s, 3, 7, 11, 15);
        quarter_round(&mut s, 0, 5, 10, 15);
        quarter_round(&mut s, 1, 6, 11, 12);
        quarter_round(&mut s, 2, 7, 8, 13);
        quarter_round(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; BLOCK_LEN];
    for (i, chunk) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        *chunk = s[i].wrapping_add(init[i]).to_le_bytes();
    }
    // Wipe the working state (it holds key material).
    s = [0; 16];
    init = [0; 16];
    core::hint::black_box((&s, &init));
    out
}

/// The block counter would wrap (RFC 8439 forbids it: the keystream would
/// repeat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterOverflow;

/// XORs `data` with the keystream starting at block `counter`
/// (RFC 8439 §2.4; encryption and decryption are the same operation).
/// The caller must never reuse a (key, nonce) pair for different data.
/// Refuses (without touching `data`) if the counter would wrap.
pub fn xor_stream(
    key: &[u8; KEY_LEN],
    counter: u32,
    nonce: &[u8; NONCE_LEN],
    data: &mut [u8],
) -> Result<(), CounterOverflow> {
    let blocks = data.len().div_ceil(BLOCK_LEN) as u64;
    if u64::from(counter) + blocks > u64::from(u32::MAX) + 1 {
        return Err(CounterOverflow);
    }
    for (i, chunk) in data.chunks_mut(BLOCK_LEN).enumerate() {
        let mut ks = block(key, counter + i as u32, nonce);
        for (d, k) in chunk.iter_mut().zip(ks.iter()) {
            *d ^= k;
        }
        crate::wipe(&mut ks);
    }
    Ok(())
}

/// Known-answer tests from RFC 8439: the block function (§2.3.2) and the
/// encryption of the "sunscreen" plaintext (§2.4.2). Used at boot.
pub fn self_test() -> bool {
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = i as u8;
    }
    // §2.3.2: nonce 00:00:00:09:00:00:00:4a:00:00:00:00, counter 1.
    let nonce: [u8; 12] = [0, 0, 0, 0x09, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let expected_block: [u8; 64] = [
        0x10, 0xf1, 0xe7, 0xe4, 0xd1, 0x3b, 0x59, 0x15, 0x50, 0x0f, 0xdd, 0x1f, 0xa3, 0x20, 0x71,
        0xc4, 0xc7, 0xd1, 0xf4, 0xc7, 0x33, 0xc0, 0x68, 0x03, 0x04, 0x22, 0xaa, 0x9a, 0xc3, 0xd4,
        0x6c, 0x4e, 0xd2, 0x82, 0x64, 0x46, 0x07, 0x9f, 0xaa, 0x09, 0x14, 0xc2, 0xd7, 0x05, 0xd9,
        0x8b, 0x02, 0xa2, 0xb5, 0x12, 0x9c, 0xd1, 0xde, 0x16, 0x4e, 0xb9, 0xcb, 0xd0, 0x83, 0xe8,
        0xa2, 0x50, 0x3c, 0x4e,
    ];
    if block(&key, 1, &nonce) != expected_block {
        return false;
    }
    // §2.4.2: nonce 00:00:00:00:00:00:00:4a:00:00:00:00, counter 1.
    let nonce: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let mut text = *b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    if xor_stream(&key, 1, &nonce, &mut text).is_err() {
        return false;
    }
    const CIPHER_START: [u8; 16] = [
        0x6e, 0x2e, 0x35, 0x9a, 0x25, 0x68, 0xf9, 0x80, 0x41, 0xba, 0x07, 0x28, 0xdd, 0x0d, 0x69,
        0x81,
    ];
    const CIPHER_END: [u8; 2] = [0x87, 0x4d];
    text[..16] == CIPHER_START && text[text.len() - 2..] == CIPHER_END
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rfc8439_vectors() {
        assert!(self_test());
    }

    #[test]
    fn round_trip() {
        let key = [7u8; 32];
        let nonce = [9u8; 12];
        let mut data = [0x42u8; 200];
        xor_stream(&key, 5, &nonce, &mut data).unwrap();
        assert_ne!(data, [0x42u8; 200]);
        xor_stream(&key, 5, &nonce, &mut data).unwrap();
        assert_eq!(data, [0x42u8; 200]);
    }

    #[test]
    fn counter_never_wraps() {
        let mut data = [0u8; 128];
        assert_eq!(
            xor_stream(&[0; 32], u32::MAX, &[0; 12], &mut data),
            Err(CounterOverflow)
        );
        assert_eq!(data, [0u8; 128]);
        assert!(xor_stream(&[0; 32], u32::MAX, &[0; 12], &mut data[..64]).is_ok());
    }
}
