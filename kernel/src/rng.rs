//! The kernel's random number generator (Phase 8a, D-066) — built on the
//! educational, not audited `kitty-crypto` (D-056).
//!
//! **Seeding.** At boot an entropy pool is hashed with SHA-256 into the
//! first key. Sources, in order of trust:
//! 1. RDSEED (the CPU's entropy source),
//! 2. RDRAND (the CPU's DRBG, reseeded by its own entropy source),
//! 3. timing jitter (TSC deltas of a short busy loop),
//! 4. interrupt timing collected while the system runs (reseeds).
//!
//! Every hardware value goes through a **health test**: a value equal to
//! the previous one is rejected (a stuck generator — some CPUs had RDRAND
//! bugs that returned the same value every time), and a source whose
//! values are rejected too often is distrusted. Without a trusted hardware
//! source the kernel still boots on jitter alone but says so loudly
//! (`weak`).
//!
//! **Generating.** ChaCha20 with *fast key erasure*: every request first
//! produces a fresh key from the keystream and throws the old one away,
//! then the output. Whoever later reads the generator's state learns
//! nothing about earlier outputs (backtracking resistance). Every
//! [`RESEED_INTERVAL`] requests, fresh hardware entropy and the
//! interrupt-timing pool are hashed into the key.

use core::sync::atomic::{AtomicU64, Ordering};

use kitty_crypto::{chacha20, sha256::Sha256, wipe};

use crate::arch::x86_64::cpu;
use crate::sync::IrqSpinLock;

/// Requests between two reseeds.
pub const RESEED_INTERVAL: u64 = 1024;
/// Hardware words wanted at boot (256 bits from RDSEED, or 512 from
/// RDRAND, which is credited at half).
const BOOT_WORDS: usize = 8;

struct Generator {
    key: [u8; 32],
    /// Distinct nonce per request (never repeats for one key, and keys
    /// never repeat).
    requests: u64,
}

static RNG: IrqSpinLock<Option<Generator>> = IrqSpinLock::new(None);
/// Interrupt timing folded in by `add_interrupt_timing` (no lock).
static IRQ_POOL: AtomicU64 = AtomicU64::new(0);

/// What seeded the generator (for the boot log; no secret values).
#[derive(Clone, Copy, Debug, Default)]
pub struct SeedReport {
    /// Good RDSEED words.
    pub rdseed: usize,
    /// Good RDRAND words.
    pub rdrand: usize,
    /// Hardware words rejected by the health test.
    pub rejected: usize,
    /// Jitter samples mixed in.
    pub jitter: usize,
    /// No trusted hardware source: seeded from jitter only.
    pub weak: bool,
}

/// Health test state for one source: rejects a value equal to the last.
struct Health {
    last: Option<u64>,
    rejected: usize,
}

impl Health {
    fn accept(&mut self, v: u64) -> bool {
        let repeated = self.last == Some(v);
        self.last = Some(v);
        if repeated {
            self.rejected += 1;
        }
        !repeated
    }
}

/// Collects `want` good words from `source` (at most 4 × `want` tries).
fn collect(
    hash: &mut Sha256,
    want: usize,
    mut source: impl FnMut() -> Option<u64>,
) -> (usize, usize) {
    let mut health = Health {
        last: None,
        rejected: 0,
    };
    let mut good = 0;
    for _ in 0..want * 4 {
        if good == want {
            break;
        }
        let Some(v) = source() else { break };
        if health.accept(v) {
            hash.update(&v.to_le_bytes());
            good += 1;
        }
    }
    (good, health.rejected)
}

/// Seeds the generator. Call once, early (before any user process).
pub fn init() -> SeedReport {
    let mut hash = Sha256::new();
    hash.update(b"kitty-OS rng v1");
    let mut report = SeedReport::default();

    let (n, r) = collect(&mut hash, BOOT_WORDS, cpu::rdseed64);
    report.rdseed = n;
    report.rejected += r;
    let (n, r) = collect(&mut hash, BOOT_WORDS * 2, cpu::rdrand64);
    report.rdrand = n;
    report.rejected += r;

    // Timing jitter: always mixed in, never trusted on its own if hardware
    // is available. 256 samples of TSC deltas around a tiny busy loop.
    let mut prev = cpu::rdtsc();
    for i in 0..256u32 {
        let mut x = i;
        for _ in 0..(i % 7) + 1 {
            x = core::hint::black_box(x.wrapping_mul(2_654_435_761).rotate_left(5));
        }
        let now = cpu::rdtsc();
        hash.update(&now.wrapping_sub(prev).to_le_bytes());
        hash.update(&x.to_le_bytes());
        prev = now;
        report.jitter += 1;
    }
    // Per-boot values that differ across machines and boots.
    hash.update(&cpu::rdtsc().to_le_bytes());
    hash.update(&IRQ_POOL.load(Ordering::Relaxed).to_le_bytes());

    // Trusted if RDSEED gave 256 bits, or RDRAND gave 512 (credited half).
    report.weak = report.rdseed < BOOT_WORDS && report.rdrand < BOOT_WORDS * 2;
    let mut key = hash.finish();
    *RNG.lock() = Some(Generator { key, requests: 0 });
    wipe(&mut key);
    report
}

/// Folds an interrupt's arrival time into the reseed pool. Lock-free and
/// cheap: called from the interrupt dispatcher.
pub fn add_interrupt_timing(tsc: u64) {
    // Rotate by the low bits so equal deltas do not cancel out.
    IRQ_POOL.fetch_xor(tsc.rotate_left((tsc & 63) as u32), Ordering::Relaxed);
}

fn reseed(g: &mut Generator) {
    let mut hash = Sha256::new();
    hash.update(&g.key);
    hash.update(&IRQ_POOL.swap(0, Ordering::Relaxed).to_le_bytes());
    let _ = collect(&mut hash, 2, cpu::rdseed64);
    let _ = collect(&mut hash, 2, cpu::rdrand64);
    hash.update(&cpu::rdtsc().to_le_bytes());
    let mut fresh = hash.finish();
    g.key = fresh;
    wipe(&mut fresh);
}

/// Fills `out` with random bytes. Safe in any context (short lock; at
/// most `out.len() / 64 + 1` ChaCha20 blocks). Panics if [`init`] has not
/// run: a kernel bug, never input-driven.
pub fn fill(out: &mut [u8]) {
    let mut guard = RNG.lock();
    let Some(g) = guard.as_mut() else {
        drop(guard);
        panic!("rng: used before init");
    };
    g.requests += 1;
    if g.requests % RESEED_INTERVAL == 0 {
        reseed(g);
    }
    let mut nonce = [0u8; chacha20::NONCE_LEN];
    nonce[..8].copy_from_slice(&g.requests.to_le_bytes());
    // Block 0: the next key (fast key erasure); blocks 1..: the output.
    let mut first = chacha20::block(&g.key, 0, &nonce);
    let mut next_key = [0u8; 32];
    next_key.copy_from_slice(&first[..32]);
    for (i, chunk) in out.chunks_mut(chacha20::BLOCK_LEN).enumerate() {
        let mut ks = chacha20::block(&g.key, (i as u32).wrapping_add(1), &nonce);
        chunk.copy_from_slice(&ks[..chunk.len()]);
        wipe(&mut ks);
    }
    g.key = next_key;
    wipe(&mut next_key);
    wipe(&mut first);
}

/// A random `u64`.
pub fn u64() -> u64 {
    let mut b = [0u8; 8];
    fill(&mut b);
    u64::from_le_bytes(b)
}

/// A random value in `0..bound` without modulo bias (`bound > 0`).
pub fn below(bound: u64) -> u64 {
    if bound <= 1 {
        return 0;
    }
    // Reject the top partial range so every value is equally likely.
    let zone = u64::MAX - (u64::MAX % bound);
    loop {
        let v = u64();
        if v < zone {
            return v % bound;
        }
    }
}

/// Boot self-test: the crypto known answers (FIPS 180-4, RFC 8439), the
/// health test rejecting a stuck source, and the generator producing
/// different, non-zero outputs.
pub fn selftest() -> Result<(), &'static str> {
    if !kitty_crypto::sha256::self_test() {
        return Err("SHA-256 known-answer test failed");
    }
    if !kitty_crypto::chacha20::self_test() {
        return Err("ChaCha20 known-answer test failed");
    }
    let mut stuck_hash = Sha256::new();
    let (good, rejected) = collect(&mut stuck_hash, 8, || Some(0xFFFF_FFFF_FFFF_FFFF));
    if good != 1 || rejected == 0 {
        return Err("the health test accepted a stuck source");
    }
    let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
    fill(&mut a);
    fill(&mut b);
    if a == b || a == [0u8; 64] || b == [0u8; 64] {
        return Err("the generator repeated itself or returned zeros");
    }
    if (0..64).any(|_| below(10) >= 10) {
        return Err("below() returned a value out of range");
    }
    Ok(())
}
