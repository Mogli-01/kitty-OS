//! kitty-OS cryptography — **educational, NOT audited** (D-056, D-066).
//!
//! Written from the specifications to learn how these primitives work,
//! and checked against the **official test vectors** (FIPS 180-4 / NIST
//! examples for SHA-256, RFC 8439 for ChaCha20). Do not use it to protect
//! anything that matters: audited implementations exist for that.
//!
//! Rules (D-056):
//! - no `unsafe`, no allocation, `no_std` (the kernel and host tests use
//!   the same code);
//! - no branches or table lookups that depend on secret data: both
//!   primitives here are add-rotate-xor / bit operations only;
//! - secrets are wiped with [`wipe`] when no longer needed.

#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod chacha20;
pub mod sha256;

/// Overwrites `secret` with zeros in a way the compiler should not remove
/// (best effort without `unsafe`: the zeros are passed to `black_box`, so
/// the stores are observable).
pub fn wipe(secret: &mut [u8]) {
    for b in secret.iter_mut() {
        *b = 0;
    }
    core::hint::black_box(&*secret);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Compares two byte strings in time that depends only on their length
/// (never on where they differ). Returns whether they are equal.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    core::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq_works() {
        assert!(ct_eq(b"kitty", b"kitty"));
        assert!(!ct_eq(b"kitty", b"kittz"));
        assert!(!ct_eq(b"kitty", b"kit"));
    }

    #[test]
    fn wipe_zeroes() {
        let mut k = [0xAAu8; 32];
        wipe(&mut k);
        assert_eq!(k, [0u8; 32]);
    }
}
