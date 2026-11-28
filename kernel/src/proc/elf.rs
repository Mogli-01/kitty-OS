//! ELF64 loader for user programs (Phase 6d, docs/FASE6.md §6, D-064).
//!
//! The file is **untrusted input** (it comes from the initrd, later from
//! disk or the network). Everything is validated before a single page is
//! mapped, with checked arithmetic and no `unwrap`:
//! - ELF64, little-endian, version 1, `EM_X86_64`, and either `ET_EXEC`
//!   (fixed addresses) or `ET_DYN` = **static PIE** (Phase 8b, D-067):
//!   loaded at a random base, with only `R_X86_64_RELATIVE` relocations;
//! - program headers inside the file, 1–16 `PT_LOAD`, no `PT_INTERP`
//!   or `PT_TLS`; `PT_DYNAMIC` only in a PIE, and then without shared
//!   libraries (`DT_NEEDED`), PLT relocations or text relocations;
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
    /// Not `ET_EXEC` or `ET_DYN` (no relocatable files or core dumps).
    NotExecutable,
    /// A PIE's dynamic section or relocations are malformed or need
    /// something unsupported (shared libraries, PLT, non-RELATIVE types,
    /// a relocation outside a writable segment).
    BadRelocation,
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

/// Whether `[start, end)` touches the window where the main stack (and its
/// guard page) may be placed (security review of 6d: a segment there would
/// silently replace the guard; 8b: the stack position is random).
fn overlaps_stack(start: u64, end: u64) -> bool {
    start < super::STACK_TOP && end > super::spawn::stack_window_bottom()
}

/// Largest span (lowest to highest byte) of a PIE image.
pub const PIE_MAX_SPAN: u64 = MAX_IMAGE;
/// Lowest base a PIE may be loaded at (4 GiB: far from fixed-address
/// `ET_EXEC` images, which usually sit at 4 MiB).
pub const PIE_BASE_MIN: u64 = 0x0000_0001_0000_0000;
/// Number of possible PIE base pages (2^24: 24 bits of ASLR, a 64 GiB
/// window).
pub const PIE_BASE_PAGES: u64 = 1 << 24;
/// Most relocations accepted.
pub const MAX_RELOCS: usize = 1 << 20;

const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_PLTRELSZ: u64 = 2;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_RELAENT: u64 = 9;
const DT_TEXTREL: u64 = 22;
const DT_JMPREL: u64 = 23;
const DT_FLAGS: u64 = 30;
const DF_TEXTREL: u64 = 0x4;
const R_X86_64_NONE: u32 = 0;
const R_X86_64_RELATIVE: u32 = 8;

/// One validated `R_X86_64_RELATIVE` relocation: at `offset` (image-
/// relative, 8-aligned, inside a writable segment) store `base + addend`.
#[derive(Clone, Copy, Debug)]
struct Reloc {
    offset: u64,
    addend: u64,
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
    /// Static PIE: addresses are offsets from a base chosen at load time.
    pie: bool,
    relocs: Vec<Reloc>,
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
        let pie = match u16_at(bytes, 16)? {
            2 => false,
            3 => true,
            _ => return Err(ElfError::NotExecutable),
        };
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
        let mut dynamic: Option<(u64, u64)> = None;
        for i in 0..phnum {
            let ph = phoff + i * 56;
            match u32_at(bytes, ph)? {
                PT_LOAD => {}
                PT_DYNAMIC if pie && dynamic.is_none() => {
                    dynamic = Some((u64_at(bytes, ph + 8)?, u64_at(bytes, ph + 32)?));
                    continue;
                }
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
            // A PIE's addresses are offsets: they only need to fit the span
            // (the base keeps the whole image inside the PIE window).
            let out_of_range = if pie {
                mem_end > PIE_MAX_SPAN
            } else {
                seg.vaddr < USER_MIN || mem_end > USER_MAP_END || overlaps_stack(seg.vaddr, mem_end)
            };
            if out_of_range {
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
        let relocs = match dynamic {
            Some((offset, size)) => parse_relocs(bytes, &segments, offset, size)?,
            None => Vec::new(),
        };
        Ok(Self {
            bytes,
            entry,
            segments,
            pie,
            relocs,
        })
    }

    /// Whether this is a static PIE (loaded at a random base).
    pub fn is_pie(&self) -> bool {
        self.pie
    }

    /// Picks a random, page-aligned load base for a PIE (0 for `ET_EXEC`).
    pub fn choose_base(&self) -> u64 {
        if self.pie {
            PIE_BASE_MIN + crate::rng::below(PIE_BASE_PAGES) * PAGE
        } else {
            0
        }
    }

    /// Pages the image will occupy.
    pub fn pages(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| (s.end_page() - s.first_page()) / PAGE)
            .sum()
    }

    /// Entry point (validated to be in an executable segment), relative to
    /// `base` for a PIE.
    pub fn entry(&self, base: u64) -> u64 {
        base + self.entry
    }

    /// Copies every segment into fresh pages of `space`, at `base` (0 for
    /// `ET_EXEC`; [`Image::choose_base`] for a PIE), then applies the PIE's
    /// relocations. Nothing runs before this returns.
    pub fn load(&self, space: &mut UserSpace, base: u64) -> Result<(), ElfError> {
        if base != 0 && (!self.pie || !base.is_multiple_of(PAGE)) {
            return Err(ElfError::OutOfRange);
        }
        if self.pie {
            let end = base.checked_add(PIE_MAX_SPAN).ok_or(ElfError::OutOfRange)?;
            if base < USER_MIN || end > super::spawn::stack_window_bottom() {
                return Err(ElfError::OutOfRange);
            }
        }
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
                    .map_page(base + p, seg.perm, &page[..])
                    .map_err(ElfError::Map)?;
                p += PAGE;
            }
        }
        for r in &self.relocs {
            space
                .poke_u64(base + r.offset, base.wrapping_add(r.addend))
                .ok_or(ElfError::BadRelocation)?;
        }
        Ok(())
    }
}

/// Reads and validates a PIE's relocations from its dynamic section
/// (`offset`/`size` in the file).
fn parse_relocs(
    bytes: &[u8],
    segments: &[Segment],
    offset: u64,
    size: u64,
) -> Result<Vec<Reloc>, ElfError> {
    let start = usize::try_from(offset).map_err(|_| ElfError::BadRelocation)?;
    let len = usize::try_from(size).map_err(|_| ElfError::BadRelocation)?;
    let dynamic = bytes
        .get(start..start.checked_add(len).ok_or(ElfError::BadRelocation)?)
        .ok_or(ElfError::BadRelocation)?;
    let (mut rela, mut relasz, mut relaent) = (None, 0u64, 24u64);
    for entry in dynamic.as_chunks::<16>().0 {
        let tag = u64_at(entry, 0)?;
        let val = u64_at(entry, 8)?;
        match tag {
            DT_NULL => break,
            DT_NEEDED | DT_TEXTREL | DT_JMPREL => return Err(ElfError::BadRelocation),
            DT_PLTRELSZ if val != 0 => return Err(ElfError::BadRelocation),
            DT_FLAGS if val & DF_TEXTREL != 0 => return Err(ElfError::BadRelocation),
            DT_RELA => rela = Some(val),
            DT_RELASZ => relasz = val,
            DT_RELAENT => relaent = val,
            _ => {}
        }
    }
    let Some(rela) = rela else {
        return if relasz == 0 {
            Ok(Vec::new())
        } else {
            Err(ElfError::BadRelocation)
        };
    };
    if relaent != 24 || relasz % 24 != 0 || relasz / 24 > MAX_RELOCS as u64 {
        return Err(ElfError::BadRelocation);
    }
    // DT_RELA is an address in the image: find its bytes through the
    // segment that contains it (in the file part).
    let end = rela.checked_add(relasz).ok_or(ElfError::BadRelocation)?;
    let seg = segments
        .iter()
        .find(|s| rela >= s.vaddr && end <= s.vaddr + s.filesz)
        .ok_or(ElfError::BadRelocation)?;
    let file_start =
        usize::try_from(seg.offset + (rela - seg.vaddr)).map_err(|_| ElfError::BadRelocation)?;
    let table = bytes
        .get(file_start..file_start + relasz as usize)
        .ok_or(ElfError::BadRelocation)?;
    let mut relocs = Vec::new();
    relocs
        .try_reserve(table.len() / 24)
        .map_err(|_| ElfError::TooBig)?;
    for r in table.as_chunks::<24>().0 {
        let r_offset = u64_at(r, 0)?;
        let info = u64_at(r, 8)?;
        let addend = u64_at(r, 16)?;
        match (info & 0xFFFF_FFFF) as u32 {
            R_X86_64_NONE => continue,
            R_X86_64_RELATIVE if info >> 32 == 0 => {}
            _ => return Err(ElfError::BadRelocation),
        }
        // Only into writable memory of the image, 8-aligned (never into
        // code or read-only data, never across pages).
        let in_rw = segments.iter().any(|s| {
            s.perm == UserPerm::ReadWrite
                && r_offset >= s.vaddr
                && r_offset.saturating_add(8) <= s.vaddr + s.memsz
        });
        if !in_rw || r_offset % 8 != 0 {
            return Err(ElfError::BadRelocation);
        }
        relocs.push(Reloc {
            offset: r_offset,
            addend,
        });
    }
    Ok(relocs)
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
