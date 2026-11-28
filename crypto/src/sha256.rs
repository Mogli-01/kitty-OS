//! SHA-256 (FIPS 180-4) — educational, not audited.
//!
//! Streaming: [`Sha256::new`] → [`Sha256::update`] (any number of times)
//! → [`Sha256::finish`]. One-shot: [`sha256`].

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Size of a digest in bytes.
pub const DIGEST_LEN: usize = 32;

/// Incremental SHA-256.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    /// Total length in bytes (the padding needs it in bits, mod 2^64).
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A fresh hash.
    pub const fn new() -> Self {
        Self {
            state: H0,
            block: [0; 64],
            used: 0,
            total: 0,
        }
    }

    /// Feeds `data`.
    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.used).min(data.len());
            let (head, rest) = data.split_at(take);
            self.block[self.used..self.used + take].copy_from_slice(head);
            self.used += take;
            data = rest;
            if self.used == 64 {
                let block = self.block;
                compress(&mut self.state, &block);
                self.used = 0;
            }
        }
    }

    /// Finishes and returns the digest (the hash state is wiped).
    pub fn finish(mut self) -> [u8; DIGEST_LEN] {
        let bits = self.total.wrapping_mul(8);
        self.update(&[0x80]);
        while self.used != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; DIGEST_LEN];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *chunk = word.to_be_bytes();
        }
        out
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        // The state and the buffered block are derived from the message
        // (and, in the RNG, from secret keys): wipe them.
        self.state = [0; 8];
        core::hint::black_box(&self.state);
        crate::wipe(&mut self.block);
    }
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(*chunk);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
    // The message schedule holds message-derived words: wipe it.
    w = [0; 64];
    core::hint::black_box(&w);
}

/// One-shot SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; DIGEST_LEN] {
    let mut h = Sha256::new();
    h.update(data);
    h.finish()
}

/// Known-answer tests (NIST FIPS 180-4 examples): the empty message,
/// "abc" (one block), the 448-bit two-block message, and the same message
/// fed byte by byte (streaming). Used by the kernel at boot.
pub fn self_test() -> bool {
    const EMPTY: [u8; 32] =
        hex32("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    const ABC: [u8; 32] = hex32("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    const TWO_BLOCKS: [u8; 32] =
        hex32("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    const MSG: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    let mut streamed = Sha256::new();
    for b in MSG {
        streamed.update(core::slice::from_ref(b));
    }
    sha256(b"") == EMPTY
        && sha256(b"abc") == ABC
        && sha256(MSG) == TWO_BLOCKS
        && streamed.finish() == TWO_BLOCKS
}

/// Parses 64 hex digits at compile time (for test vectors).
pub(crate) const fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nibble(b[2 * i]) << 4) | nibble(b[2 * i + 1]);
        i += 1;
    }
    out
}

pub(crate) const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nist_vectors() {
        assert!(self_test());
    }

    #[test]
    fn million_a() {
        let mut h = Sha256::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            h.update(&chunk);
        }
        assert_eq!(
            h.finish(),
            hex32("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
        );
    }

    #[test]
    fn boundary_vectors() {
        // Independent digests (Python hashlib) of "a" repeated n times, for
        // the lengths where the padding changes shape.
        let cases: [(usize, &str); 6] = [
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                64,
                "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            ),
            (
                119,
                "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
            ),
            (
                120,
                "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c",
            ),
        ];
        let data = [b'a'; 120];
        for (n, hex) in cases {
            assert_eq!(sha256(&data[..n]), hex32(hex), "len {n}");
        }
    }

    #[test]
    fn padding_boundaries() {
        // 55, 56 and 64 bytes cross the length-field boundary differently;
        // streaming and one-shot must agree for every length up to 130.
        let data = [0x5Au8; 130];
        for len in 0..=130 {
            let mut h = Sha256::new();
            let (a, b) = data[..len].split_at(len / 3);
            h.update(a);
            h.update(b);
            assert_eq!(h.finish(), sha256(&data[..len]), "len {len}");
        }
    }
}
