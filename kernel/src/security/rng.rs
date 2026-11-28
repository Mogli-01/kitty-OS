//! Kernel policy for obtaining a hardware-derived random word.

use crate::arch::x86_64::entropy::Source;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NoEntropy;

/// Try RDSEED and then RDRAND on the boot CPU. Fails if neither source
/// supplies a word within its attempt budget. No state, timer or addresses
/// are used to generate or replace the word.
pub fn hardware_word() -> Result<u64, NoEntropy> {
    Source::Rdseed
        .read_word()
        .or_else(|_| Source::Rdrand.read_word())
        .map_err(|_| NoEntropy)
}
