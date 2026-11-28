//! Bounded reads from the x86 hardware random instructions.

use core::arch::x86_64::{__cpuid, __cpuid_count, _rdrand64_step, _rdseed64_step};

const ATTEMPTS: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Rdseed,
    Rdrand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    Unsupported,
    Unavailable,
}

impl Source {
    fn supported(self) -> bool {
        let max_leaf = __cpuid(0).eax;
        match self {
            Self::Rdseed => max_leaf >= 7 && __cpuid_count(7, 0).ebx & (1 << 18) != 0,
            Self::Rdrand => max_leaf >= 1 && __cpuid(1).ecx & (1 << 30) != 0,
        }
    }

    /// Read one hardware word on the current CPU. Each call tries its selected
    /// instruction at most ten times. The caller chooses the source and any
    /// fallback policy. This module carries no generator state.
    pub fn read_word(self) -> Result<u64, ReadError> {
        if !self.supported() {
            return Err(ReadError::Unsupported);
        }

        retry(|word| match self {
            // SAFETY: CPUID on this CPU reported RDSEED above. The only
            // output is a write to the local word through a valid borrow.
            Self::Rdseed => unsafe { _rdseed64_step(word) },
            // SAFETY: CPUID on this CPU reported RDRAND above. The only
            // output is a write to the local word through a valid borrow.
            Self::Rdrand => unsafe { _rdrand64_step(word) },
        })
    }
}

fn retry(mut step: impl FnMut(&mut u64) -> i32) -> Result<u64, ReadError> {
    let mut word = 0;
    for _ in 0..ATTEMPTS {
        if step(&mut word) == 1 {
            return Ok(word);
        }
    }
    Err(ReadError::Unavailable)
}

/// Checks the retry policy and reports only status from actual CPU reads.
#[cfg(feature = "selftest-entropy")]
pub(super) fn selftest() {
    let mut attempts = 0;
    let unavailable = retry(|word| {
        attempts += 1;
        *word = 42;
        0
    });
    assert_eq!(attempts, 10);
    assert_eq!(unavailable, Err(ReadError::Unavailable));

    let mut attempts = 0;
    let ready = retry(|word| {
        attempts += 1;
        if attempts == 10 {
            *word = 0;
            1
        } else {
            *word = 42;
            0
        }
    });
    assert_eq!(attempts, 10);
    assert_eq!(ready, Ok(0));
    crate::kprintln!("[entropy] retry self-test passed");

    for (name, source) in [("RDSEED", Source::Rdseed), ("RDRAND", Source::Rdrand)] {
        let status = match source.read_word() {
            Ok(_) => "ready",
            Err(ReadError::Unsupported) => "unsupported",
            Err(ReadError::Unavailable) => "unavailable",
        };
        crate::kprintln!("[entropy] {}: {}", name, status);
    }
}
