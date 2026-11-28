//! ELF64 loader for user programs (Phase 6d, docs/FASE6.md §6, D-064).
//!
//! The file is **untrusted input** (it comes from the initrd, later from
//! disk or the network). Everything is validated before a single page is
//! mapped, with checked arithmetic and no `unwrap`:
//! - ELF64, little-endian, version 1, `ET_EXEC`, `EM_X86_64`;
//! - program headers inside the file, 1–16 `PT_LOAD`, no `PT_INTERP` /
//!   `PT_DYNAMIC` / `PT_TLS` (static executables only);
//! - each segment: `filesz <= memsz`, file bytes inside the file,
//!   `vaddr % 4096 == offset % 4096`, whole page range inside
//!   `[USER_MIN, USER_MAP_END)`, **never writable and executable**;
//! - segments never share a page (each page gets exactly one permission);
//! - the entry point inside an executable segment;
//! - at most [`MAX_IMAGE`] bytes of memory in total.
//!
//! Loading **copies** the bytes into fresh, zeroed frames (the initrd is
//! read-only and shared; the program must not be able to change it), and
//! the parts of a page not covered by the file stay zero (`.bss`).

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::mm::paging::{MapError, USER_MAP_END, USER_MIN};
use crate::mm::uspace::{UserPerm, UserSpace};

const PAGE: u64 = 4096;
/// Most program headers accepted.
pub const MAX_PHDRS: usize = 64;
/// Most loadable segments accepted.
pub const MAX_LOAD: usize = 16;
/// Most memory an image may ask for (sum of its segments).
pub const MAX_IMAGE: u64 = 64 * 1024 * 1024;

const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_TLS: u32 = 7;
const PF_X: u32 = 1;
const PF_W: u32 = 2;

/// Why an image was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    /// Shorter than its headers say.
    Truncated,
    /// Not `\x7fELF`.
    BadMagic,
    /// Not 64-bit, little-endian, version 1, x86_64.
    Unsupported,
    /// Not `ET_EXEC` (no shared objects or relocatable files).
    NotExecutable,
    /// Program headers malformed or too many.
    BadProgramHeaders,
    /// Interpreter, dynamic section or TLS: not supported.
    Dynamic,
    /// No loadable segment, or more than [`MAX_LOAD`].
    BadSegmentCount,
    /// A segment whose sizes or offsets do not add up.
    BadSegment,
    /// A segment outside the user range (or below 1 MiB).
    OutOfRange,
    /// A segment both writable and executable.
    WriteAndExecute,
    /// Two segments share a page.
    Overlap,
    /// Entry point not inside an executable segment.
    BadEntry,
    /// The image asks for more than [`MAX_IMAGE`].
    TooBig,
    /// Mapping failed (out of memory).
    Map(MapError),
}

/// Whether `[start, end)` touches any possible main stack or guard page.
fn overlaps_stack(start: u64, end: u64) -> bool {
    start < super::STACK_TOP && end > super::spawn::STACK_WINDOW_START
}

/// One validated loadable segment.
#[derive(Clone, Copy, Debug)]
struct Segment {
    vaddr: u64,
    memsz: u64,
    offset: u64,
    filesz: u64,
    perm: UserPerm,
}

impl Segment {
    fn first_page(&self) -> u64 {
        self.vaddr & !(PAGE - 1)
    }

    /// One past the last page (validated not to overflow).
    fn end_page(&self) -> u64 {
        (self.vaddr + self.memsz).next_multiple_of(PAGE)
    }
}

/// A validated image, ready to be loaded.
#[derive(Debug)]
pub struct Image<'a> {
    bytes: &'a [u8],
    entry: u64,
    segments: Vec<Segment>,
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, ElfError> {
    let s = b.get(at..at + 2).ok_or(ElfError::Truncated)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ElfError> {
    let s = b.get(at..at + 4).ok_or(ElfError::Truncated)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Ok(u32::from_le_bytes(a))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, ElfError> {
    let s = b.get(at..at + 8).ok_or(ElfError::Truncated)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

impl<'a> Image<'a> {
    /// Validates `bytes` as a static x86_64 executable.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ElfError> {
        if bytes.len() < 64 {
            return Err(ElfError::Truncated);
        }
        if bytes.get(0..4) != Some(b"\x7fELF".as_slice()) {
            return Err(ElfError::BadMagic);
        }
        // EI_CLASS = 2 (64-bit), EI_DATA = 1 (little-endian), EI_VERSION = 1.
        if bytes.get(4..7) != Some([2u8, 1, 1].as_slice()) {
            return Err(ElfError::Unsupported);
        }
        if u16_at(bytes, 18)? != 62 || u32_at(bytes, 20)? != 1 {
            return Err(ElfError::Unsupported);
        }
        if u16_at(bytes, 16)? != 2 {
            return Err(ElfError::NotExecutable);
        }
        let entry = u64_at(bytes, 24)?;
        let phoff = usize::try_from(u64_at(bytes, 32)?).map_err(|_| ElfError::BadProgramHeaders)?;
        let phentsize = usize::from(u16_at(bytes, 54)?);
        let phnum = usize::from(u16_at(bytes, 56)?);
        if phentsize != 56 || phnum == 0 || phnum > MAX_PHDRS {
            return Err(ElfError::BadProgramHeaders);
        }
        let table_end = phnum
            .checked_mul(56)
            .and_then(|n| n.checked_add(phoff))
            .ok_or(ElfError::BadProgramHeaders)?;
        if table_end > bytes.len() {
            return Err(ElfError::BadProgramHeaders);
        }

        let mut segments = Vec::new();
        let mut total: u64 = 0;
        for i in 0..phnum {
            let ph = phoff + i * 56;
            match u32_at(bytes, ph)? {
                PT_LOAD => {}
                PT_INTERP | PT_DYNAMIC | PT_TLS => return Err(ElfError::Dynamic),
                _ => continue,
            }
            let flags = u32_at(bytes, ph + 4)?;
            let seg = Segment {
                offset: u64_at(bytes, ph + 8)?,
                vaddr: u64_at(bytes, ph + 16)?,
                filesz: u64_at(bytes, ph + 32)?,
                memsz: u64_at(bytes, ph + 40)?,
                perm: match (flags & PF_W != 0, flags & PF_X != 0) {
                    (true, true) => return Err(ElfError::WriteAndExecute),
                    (true, false) => UserPerm::ReadWrite,
                    (false, true) => UserPerm::ReadExec,
                    (false, false) => UserPerm::Read,
                },
            };
            if seg.memsz == 0 || seg.filesz > seg.memsz {
                return Err(ElfError::BadSegment);
            }
            let file_end = seg
                .offset
                .checked_add(seg.filesz)
                .ok_or(ElfError::BadSegment)?;
            if file_end > bytes.len() as u64 || seg.vaddr % PAGE != seg.offset % PAGE {
                return Err(ElfError::BadSegment);
            }
            let mem_end = seg
                .vaddr
                .checked_add(seg.memsz)
                .ok_or(ElfError::OutOfRange)?;
            // USER_MAP_END is page-aligned: `mem_end <= USER_MAP_END` keeps
            // the rounded-up last page inside too (and cannot overflow).
            if seg.vaddr < USER_MIN || mem_end > USER_MAP_END || overlaps_stack(seg.vaddr, mem_end)
            {
                return Err(ElfError::OutOfRange);
            }
            total = total.saturating_add(seg.end_page() - seg.first_page());
            if total > MAX_IMAGE {
                return Err(ElfError::TooBig);
            }
            if segments.len() == MAX_LOAD {
                return Err(ElfError::BadSegmentCount);
            }
            segments.try_reserve(1).map_err(|_| ElfError::TooBig)?;
            segments.push(seg);
        }
        if segments.is_empty() {
            return Err(ElfError::BadSegmentCount);
        }
        segments.sort_unstable_by_key(|s| s.vaddr);
        for pair in segments.windows(2) {
            if let [a, b] = pair
                && a.end_page() > b.first_page()
            {
                return Err(ElfError::Overlap);
            }
        }
        let entry_ok = segments
            .iter()
            .any(|s| s.perm == UserPerm::ReadExec && (s.vaddr..s.vaddr + s.memsz).contains(&entry));
        if !entry_ok {
            return Err(ElfError::BadEntry);
        }
        Ok(Self {
            bytes,
            entry,
            segments,
        })
    }

    /// Pages the image will occupy.
    pub fn pages(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| (s.end_page() - s.first_page()) / PAGE)
            .sum()
    }

    /// Entry point (validated to be in an executable segment).
    pub fn entry(&self) -> u64 {
        self.entry
    }

    /// Copies every segment into fresh pages of `space`.
    pub fn load(&self, space: &mut UserSpace) -> Result<(), ElfError> {
        let mut page = Box::new([0u8; PAGE as usize]);
        for seg in &self.segments {
            let file = self
                .bytes
                .get(seg.offset as usize..(seg.offset + seg.filesz) as usize)
                .ok_or(ElfError::BadSegment)?;
            let mut p = seg.first_page();
            while p < seg.end_page() {
                page.fill(0);
                // File bytes of this segment that land in [p, p + PAGE).
                let lo = p.max(seg.vaddr);
                let hi = (p + PAGE).min(seg.vaddr + seg.filesz);
                if lo < hi {
                    let src = file
                        .get((lo - seg.vaddr) as usize..(hi - seg.vaddr) as usize)
                        .ok_or(ElfError::BadSegment)?;
                    let at = (lo - p) as usize;
                    page.get_mut(at..at + src.len())
                        .ok_or(ElfError::BadSegment)?
                        .copy_from_slice(src);
                }
                space
                    .map_page(p, seg.perm, &page[..])
                    .map_err(ElfError::Map)?;
                p += PAGE;
            }
        }
        Ok(())
    }
}

/// Builds a minimal valid ELF64 image (for self-tests): one `PT_LOAD` per
/// `(vaddr, flags, bytes, memsz)`, entry `entry`. Segment `i` is placed at
/// file offset `0x1000 * (i + 1)` so `vaddr % 4096 == offset % 4096` holds
/// for page-aligned `vaddr`.
pub fn build_test_elf(segments: &[(u64, u32, &[u8], u64)], entry: u64) -> Vec<u8> {
    let phnum = segments.len();
    let end = 0x1000 * (phnum + 1) + 0x1000;
    let mut out = alloc::vec![0u8; end];
    let mut put = |at: usize, v: &[u8]| {
        if let Some(d) = out.get_mut(at..at + v.len()) {
            d.copy_from_slice(v);
        }
    };
    put(0, b"\x7fELF");
    put(4, &[2, 1, 1]);
    put(16, &2u16.to_le_bytes());
    put(18, &62u16.to_le_bytes());
    put(20, &1u32.to_le_bytes());
    put(24, &entry.to_le_bytes());
    put(32, &64u64.to_le_bytes());
    put(52, &64u16.to_le_bytes());
    put(54, &56u16.to_le_bytes());
    put(56, &(phnum as u16).to_le_bytes());
    for (i, (vaddr, flags, bytes, memsz)) in segments.iter().enumerate() {
        let ph = 64 + i * 56;
        let offset = (0x1000 * (i + 1)) as u64 + (vaddr % PAGE);
        put(ph, &PT_LOAD.to_le_bytes());
        put(ph + 4, &flags.to_le_bytes());
        put(ph + 8, &offset.to_le_bytes());
        put(ph + 16, &vaddr.to_le_bytes());
        put(ph + 32, &(bytes.len() as u64).to_le_bytes());
        put(ph + 40, &memsz.to_le_bytes());
        put(ph + 48, &PAGE.to_le_bytes());
        put(offset as usize, bytes);
    }
    out
}
