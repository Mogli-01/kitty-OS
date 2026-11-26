//! Safe, architecture-independent view of what the bootloader handed us.
//!
//! The arch layer (`arch::x86_64::boot`) is the only place that reads raw
//! Limine structures; it validates them and builds these types. Everything
//! else in the kernel only ever sees validated data.

pub use crate::arch::x86_64::boot::MemoryMap;

/// Information collected from the bootloader at entry.
pub struct BootInfo {
    /// Offset of the higher-half direct map: physical address `p` is
    /// readable at virtual address `p + hhdm_offset`. This is a kernel
    /// address: never print it.
    pub hhdm_offset: u64,
    /// Physical memory map.
    pub memory_map: MemoryMap,
    /// Where the kernel image was loaded.
    pub kernel_image: KernelImage,
    /// Physical address of the ACPI RSDP, if the firmware has one.
    pub rsdp_phys: Option<u64>,
    /// The boot module tagged `kitty-initrd`, as reported by the
    /// bootloader (not validated yet: see `mm::vmm::initrd_slice`).
    pub initrd: InitrdModule,
    /// The first usable linear framebuffer, if any.
    pub framebuffer: Option<FrameBuffer>,
    /// Bootloader name and version, if reported.
    pub bootloader: Option<BootloaderInfo>,
}

/// What the bootloader reported for the initrd module (docs/INTERFACES.md,
/// "initrd"). Only raw values: the kernel validates them against the memory
/// map before creating any slice (D-043).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitrdModule {
    /// No module carries the `kitty-initrd` string.
    Absent,
    /// Exactly one module carries it.
    Found {
        /// Virtual (HHDM) address reported by the bootloader.
        virt: u64,
        /// Size in bytes reported by the bootloader.
        size: u64,
    },
    /// More than one module carries it: refused (which one is meant?).
    Ambiguous,
}

/// Physical and virtual load address of the kernel image.
#[derive(Clone, Copy)]
pub struct KernelImage {
    /// Physical address of the first byte of the image.
    pub phys_base: u64,
    /// Virtual address it is linked/mapped at.
    pub virt_base: u64,
}

/// What a physical memory region is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryKind {
    /// Free RAM the kernel may allocate.
    Usable,
    /// RAM holding bootloader data (our boot stack, page tables, the memory
    /// map itself). Becomes usable once nothing references it.
    BootloaderReclaimable,
    /// The kernel image and boot modules.
    KernelAndModules,
    /// ACPI tables; reusable after they have been parsed.
    AcpiReclaimable,
    /// ACPI non-volatile storage: never touch.
    AcpiNvs,
    /// The linear framebuffer.
    Framebuffer,
    /// Defective RAM.
    Bad,
    /// Anything else (firmware, MMIO holes, …).
    Reserved,
}

/// One entry of the physical memory map.
#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    /// Physical start address.
    pub base: u64,
    /// Length in bytes.
    pub len: u64,
    /// Usage of the region.
    pub kind: MemoryKind,
}

/// Name and version string of the bootloader.
#[derive(Clone, Copy)]
pub struct BootloaderInfo {
    /// Bootloader name, e.g. "Limine".
    pub name: &'static str,
    /// Bootloader version string.
    pub version: &'static str,
}

/// Position and width (in bits) of one colour channel inside a pixel.
#[derive(Clone, Copy, Debug)]
pub struct ChannelMask {
    /// Bit offset of the channel's least significant bit.
    pub shift: u8,
    /// Number of bits in the channel (at most 8).
    pub size: u8,
}

/// A validated 32-bit-per-pixel linear framebuffer.
///
/// Holding a `FrameBuffer` means exclusive access to the pixel memory: the
/// type owns a `&'static mut [u8]`, so the borrow checker prevents aliasing.
pub struct FrameBuffer {
    pixels: &'static mut [u8],
    width: usize,
    height: usize,
    pitch: usize,
    red: ChannelMask,
    green: ChannelMask,
    blue: ChannelMask,
}

/// Bytes per pixel. Only 32 bpp framebuffers are accepted.
pub const BYTES_PER_PIXEL: usize = 4;

impl FrameBuffer {
    /// Validates the geometry and wraps the pixel memory.
    ///
    /// Returns `None` if the geometry does not fit in `pixels` or a channel
    /// mask cannot be represented in 32 bits. After this check every pixel
    /// coordinate `(x < width, y < height)` is guaranteed to be in bounds.
    pub fn new(
        pixels: &'static mut [u8],
        width: usize,
        height: usize,
        pitch: usize,
        red: ChannelMask,
        green: ChannelMask,
        blue: ChannelMask,
    ) -> Option<Self> {
        let row_bytes = width.checked_mul(BYTES_PER_PIXEL)?;
        let total = pitch.checked_mul(height)?;
        let mask_ok = |m: ChannelMask| m.size <= 8 && u32::from(m.shift) + u32::from(m.size) <= 32;
        if width == 0
            || height == 0
            || pitch < row_bytes
            || total > pixels.len()
            || !mask_ok(red)
            || !mask_ok(green)
            || !mask_ok(blue)
        {
            return None;
        }
        Some(Self {
            pixels,
            width,
            height,
            pitch,
            red,
            green,
            blue,
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Converts an 8-bit-per-channel RGB colour into the native pixel value.
    pub fn color(&self, r: u8, g: u8, b: u8) -> u32 {
        let chan = |v: u8, m: ChannelMask| {
            if m.size == 0 {
                0
            } else {
                (u32::from(v) >> (8 - m.size)) << m.shift
            }
        };
        chan(r, self.red) | chan(g, self.green) | chan(b, self.blue)
    }

    /// Writes one pixel. Out-of-range coordinates are ignored.
    #[inline]
    pub fn put_pixel(&mut self, x: usize, y: usize, value: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let off = y * self.pitch + x * BYTES_PER_PIXEL;
        if let Some(px) = self.pixels.get_mut(off..off + BYTES_PER_PIXEL) {
            px.copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Fills a rectangle, clipped to the screen.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, value: u32) {
        let x_end = x.saturating_add(w).min(self.width);
        let y_end = y.saturating_add(h).min(self.height);
        let bytes = value.to_le_bytes();
        for row in y..y_end {
            let start = row * self.pitch + x * BYTES_PER_PIXEL;
            let end = row * self.pitch + x_end * BYTES_PER_PIXEL;
            if let Some(line) = self.pixels.get_mut(start..end) {
                for px in line.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
                    *px = bytes;
                }
            }
        }
    }

    /// Moves the whole picture up by `rows` pixel rows and clears the freed
    /// rows at the bottom with `fill`.
    pub fn scroll_up(&mut self, rows: usize, fill: u32) {
        let rows = rows.min(self.height);
        let visible = self.pitch * self.height;
        let shift = self.pitch * rows;
        self.pixels.copy_within(shift..visible, 0);
        let width = self.width;
        let height = self.height;
        self.fill_rect(0, height - rows, width, rows, fill);
    }
}
