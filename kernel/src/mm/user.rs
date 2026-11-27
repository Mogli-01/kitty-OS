//! Safe access to user memory: `UserSlice`, `UserPtr`, `copy_from_user`,
//! `copy_to_user` (Phase 6a, docs/FASE6.md §4, D-050).
//!
//! The kernel never dereferences a user pointer. A user address is wrapped
//! in a [`UserSlice`]/[`UserPtr`], which is only constructible if the whole
//! range lies in the user half (no overflow, below
//! `0x0000_8000_0000_0000`), and is only read or written through the
//! functions below. A range that is valid but not mapped (or not accessible)
//! returns [`UserError::Fault`] thanks to the #PF fixup in
//! `arch::x86_64::usercopy` — never a panic.
//!
//! Copy once, then validate: callers copy user data into a kernel buffer
//! and only then look at it, so a program cannot change the bytes between
//! the kernel's check and its use (no TOCTOU / double fetch).

use crate::arch::x86_64::usercopy;

/// First address of the kernel (upper) half; user ranges must end at or
/// below it.
const USER_END: u64 = 0x0000_8000_0000_0000;
/// Largest single copy (a syscall argument should never need more).
pub const MAX_COPY: usize = 1 << 20;

/// Why a user memory access failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserError {
    /// The range is not entirely in user space, overflows, or is too big.
    BadRange,
    /// The range is valid but (part of) it is not mapped or not accessible
    /// to the process: the syscall returns `E_FAULT`.
    Fault,
}

/// A validated range of user memory (not yet accessed).
#[derive(Clone, Copy, Debug)]
pub struct UserSlice {
    addr: u64,
    len: usize,
}

impl UserSlice {
    /// Validates that `[addr, addr + len)` lies in the user half and that
    /// `len <= MAX_COPY`. Does not check that the memory is mapped: that is
    /// discovered (safely) by the copy itself.
    pub fn new(addr: u64, len: usize) -> Result<Self, UserError> {
        if len > MAX_COPY {
            return Err(UserError::BadRange);
        }
        let end = addr.checked_add(len as u64).ok_or(UserError::BadRange)?;
        if end > USER_END {
            return Err(UserError::BadRange);
        }
        Ok(Self { addr, len })
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the slice is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Copies the user range `src` into `dst` (same length, else `BadRange`).
pub fn copy_from_user(dst: &mut [u8], src: UserSlice) -> Result<(), UserError> {
    if dst.len() != src.len {
        return Err(UserError::BadRange);
    }
    if src.len == 0 {
        return Ok(());
    }
    let user = core::ptr::with_exposed_provenance::<u8>(src.addr as usize);
    // SAFETY: `dst` is a valid, exclusively borrowed kernel buffer of
    // `src.len` bytes; `src` was validated by `UserSlice::new` to lie
    // entirely in the user half, so it cannot overlap `dst` (kernel memory)
    // and any fault on it is recovered by the #PF fixup.
    let ok = unsafe { usercopy::copy(dst.as_mut_ptr(), user, src.len) };
    ok.then_some(()).ok_or(UserError::Fault)
}

/// Copies `src` into the user range `dst` (same length, else `BadRange`).
pub fn copy_to_user(dst: UserSlice, src: &[u8]) -> Result<(), UserError> {
    if src.len() != dst.len {
        return Err(UserError::BadRange);
    }
    if dst.len == 0 {
        return Ok(());
    }
    let user = core::ptr::with_exposed_provenance_mut::<u8>(dst.addr as usize);
    // SAFETY: `src` is a valid kernel buffer of `dst.len` bytes; `dst` was
    // validated to lie entirely in the user half (no overlap with `src`),
    // and any fault on it is recovered by the #PF fixup.
    let ok = unsafe { usercopy::copy(user, src.as_ptr(), dst.len) };
    ok.then_some(()).ok_or(UserError::Fault)
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
}

/// Plain integer types that can be read from / written to user memory.
/// Every bit pattern is a valid value, so copying raw bytes is sound.
pub trait UserValue: sealed::Sealed + Copy {
    /// Size in bytes.
    const SIZE: usize;
    /// From little-endian bytes (`bytes.len() == SIZE`).
    fn from_bytes(bytes: &[u8]) -> Self;
    /// To little-endian bytes, written into `out` (`out.len() == SIZE`).
    fn to_bytes(self, out: &mut [u8]);
}

macro_rules! user_value {
    ($($t:ty),*) => {$(
        impl UserValue for $t {
            const SIZE: usize = core::mem::size_of::<$t>();
            fn from_bytes(bytes: &[u8]) -> Self {
                let mut raw = [0u8; core::mem::size_of::<$t>()];
                raw.copy_from_slice(bytes);
                <$t>::from_le_bytes(raw)
            }
            fn to_bytes(self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}
user_value!(u8, u16, u32, u64, i32, i64);

/// A validated user pointer to one `T`.
#[derive(Clone, Copy, Debug)]
pub struct UserPtr<T: UserValue> {
    slice: UserSlice,
    _type: core::marker::PhantomData<T>,
}

impl<T: UserValue> UserPtr<T> {
    /// Validates the pointer (range in user space). Unaligned pointers are
    /// allowed (the copy is byte-wise).
    pub fn new(addr: u64) -> Result<Self, UserError> {
        Ok(Self {
            slice: UserSlice::new(addr, T::SIZE)?,
            _type: core::marker::PhantomData,
        })
    }

    /// Reads the value (one copy).
    pub fn read(self) -> Result<T, UserError> {
        let mut buf = [0u8; 8];
        let bytes = buf.get_mut(..T::SIZE).ok_or(UserError::BadRange)?;
        copy_from_user(bytes, self.slice)?;
        Ok(T::from_bytes(bytes))
    }

    /// Writes the value.
    pub fn write(self, value: T) -> Result<(), UserError> {
        let mut buf = [0u8; 8];
        let bytes = buf.get_mut(..T::SIZE).ok_or(UserError::BadRange)?;
        value.to_bytes(bytes);
        copy_to_user(self.slice, bytes)
    }
}

// ------------------------------------------------------------ self-tests ---

use super::addr::{PAGE_SIZE, VirtAddr};
use super::paging::{CacheType, MapFlags};
use super::pmm::{self, PhysFrame};
use super::vmm;

/// Address of the temporary user page used by the self-tests (16 MiB,
/// lower half). Nothing else lives in the lower half yet.
const TEST_PAGE: u64 = 0x0000_0000_0100_0000;
/// A user address that is never mapped.
const UNMAPPED: u64 = 0x0000_0000_0200_0000;

fn user_flags(writable: bool, executable: bool) -> MapFlags {
    MapFlags {
        writable,
        executable,
        user: true,
        global: false,
        cache: CacheType::WriteBack,
    }
}

/// Maps one fresh (zeroed) frame at `TEST_PAGE` with user `flags`, runs
/// `f`, then unmaps it and frees the frame.
fn with_test_page<R>(flags: MapFlags, fill: &[u8], f: impl FnOnce() -> R) -> Option<R> {
    let frame = pmm::alloc_frame()?;
    // Prepare the contents through the kernel's own mapping of the frame.
    let kernel_view = super::hhdm::phys_to_virt(frame.addr()).as_u64() as *mut u8;
    let n = fill.len().min(PAGE_SIZE as usize);
    // SAFETY: the frame was just allocated (exclusively ours, zeroed) and is
    // mapped writable in the HHDM; we write at most one page.
    unsafe { core::ptr::copy_nonoverlapping(fill.as_ptr(), kernel_view, n) };
    let virt = VirtAddr::new(TEST_PAGE)?;
    let phys = frame.addr();
    vmm::with_kernel_space(|s| s.map_4k(virt, phys, flags)).and_then(Result::ok)?;
    let result = f();
    match vmm::with_kernel_space(|s| s.unmap_4k(virt)) {
        // Unmapped: the frame can go back to the PMM.
        Some(Ok(_)) => drop::<PhysFrame>(frame),
        // Still mapped with the user bit: never free it, or a user-visible
        // alias of reused memory would remain. Leak it instead.
        _ => core::mem::forget(frame),
    }
    Some(result)
}

/// Boot self-test of the user-copy path: round trip through a real user
/// page, fault recovery on an unmapped user address, and refusal of
/// kernel-half ranges. Returns whether everything behaved.
pub fn selftest() -> bool {
    let rw = user_flags(true, false);
    let result = with_test_page(rw, &[], || {
        let Ok(dst) = UserSlice::new(TEST_PAGE + 100, 5) else {
            return false;
        };
        let wrote = copy_to_user(dst, b"kitty").is_ok();
        let mut back = [0u8; 5];
        let read = copy_from_user(&mut back, dst).is_ok() && &back == b"kitty";
        let value_ok = UserPtr::<u64>::new(TEST_PAGE + 3)
            .and_then(|p| p.write(0x1122_3344_5566_7788).and_then(|()| p.read()))
            == Ok(0x1122_3344_5566_7788);
        // A range that starts in the page and runs off its end must fault
        // (the next page is not mapped), not crash.
        let straddle =
            UserSlice::new(TEST_PAGE + PAGE_SIZE - 2, 4).map(|s| copy_from_user(&mut [0u8; 4], s));
        wrote && read && value_ok && straddle == Ok(Err(UserError::Fault))
    })
    .unwrap_or(false);

    let unmapped = UserSlice::new(UNMAPPED, 8).map(|s| copy_from_user(&mut [0u8; 8], s))
        == Ok(Err(UserError::Fault));
    let kernel_half = UserSlice::new(0xFFFF_8000_0000_0000, 8).is_err()
        && UserSlice::new(USER_END - 4, 8).is_err()
        && UserSlice::new(u64::MAX - 2, 8).is_err();
    result && unmapped && kernel_half
}

/// Feature self-test: the kernel reads a user page **directly** (not via
/// `copy_from_user`). With SMAP this must fault: expect "protection
/// violation on read in kernel mode".
#[cfg(feature = "selftest-smap")]
pub fn smap_violation() {
    let _ = with_test_page(user_flags(true, false), &[], || {
        // SAFETY: deliberately reads a user page without `stac`, to prove
        // SMAP stops it; the #PF handler reports and panics. One `mov` in
        // asm without `nomem`/`nostack` (lesson of issue #7).
        unsafe {
            core::arch::asm!("mov {tmp}, qword ptr [{addr}]", addr = in(reg) TEST_PAGE, tmp = out(reg) _);
        }
    });
}

/// Feature self-test: the kernel jumps into an executable **user** page
/// containing `ret`. With SMEP this must fault: expect "protection
/// violation on instruction fetch in kernel mode".
#[cfg(feature = "selftest-smep")]
pub fn smep_violation() {
    let _ = with_test_page(user_flags(false, true), &[0xC3], || {
        // SAFETY: deliberately executes a user page to prove SMEP stops it;
        // the #PF handler reports and panics before anything runs there.
        unsafe {
            core::arch::asm!("call {addr}", addr = in(reg) TEST_PAGE, clobber_abi("C"));
        }
    });
}
