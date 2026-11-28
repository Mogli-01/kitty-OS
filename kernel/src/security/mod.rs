//! Core security features: CPU protections (NX, SMEP, SMAP, UMIP), KASLR,
//! kernel RNG and per-process capabilities (Phases 3, 6 and 8).
//! See docs/SECURITY.md for the status of each protection.

pub mod rng;
