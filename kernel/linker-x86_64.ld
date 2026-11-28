/*
 * kitty-OS kernel linker script (x86_64).
 *
 * Every section group gets its own PT_LOAD segment with exact permissions.
 * Limine maps each segment with the flags we declare here, which gives us
 * W^X from the very first instruction:
 *
 *   requests  R--   Limine boot protocol requests (written only by the
 *                   bootloader *before* paging is handed to us)
 *   text      R-X   code
 *   rodata    R--   constants, strings, font, .data.rel.ro, .got and the
 *                   dynamic/relocation tables (KASLR, Phase 8c)
 *   data      RW-   .data + .bss (NX)
 *
 * The kernel is a PIE (D-068): Limine picks a random base and applies the
 * R_X86_64_RELATIVE relocations in .rela.dyn before jumping to us, while
 * the memory is still writable for it; for the kernel it is read-only.
 *
 * Each segment starts on a 4 KiB page boundary so permissions never have to
 * be merged inside one page.
 */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(kernel_entry)

PHDRS
{
    requests PT_LOAD FLAGS(4);       /* R   */
    text     PT_LOAD FLAGS(5);       /* R+X */
    rodata   PT_LOAD FLAGS(4);       /* R   */
    data     PT_LOAD FLAGS(6);       /* R+W */
    dynamic  PT_DYNAMIC FLAGS(4);    /* relocation info for KASLR (8c) */
}

SECTIONS
{
    /* Top 2 GiB of the address space: required by -C code-model=kernel. */
    . = 0xffffffff80000000;
    __kernel_start = .;

    .requests : {
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
    } :requests

    . = ALIGN(4K);
    __text_start = .;
    .text : {
        *(.text .text.*)
    } :text
    __text_end = .;

    . = ALIGN(4K);
    __rodata_start = .;
    .rodata : {
        *(.rodata .rodata.*)
        /* Data with absolute addresses that only the bootloader writes
         * (relocations, Phase 8c): read-only for the kernel. */
        *(.data.rel.ro .data.rel.ro.*)
    } :rodata
    /* Relocation and dynamic tables Limine uses to slide the kernel
     * (KASLR): read-only. */
    .dynsym   : { *(.dynsym) }   :rodata
    .dynstr   : { *(.dynstr) }   :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .hash     : { *(.hash) }     :rodata
    .rela.dyn : { *(.rela.dyn .rela.*) } :rodata
    /* Absolute addresses, filled in by the bootloader: read-only for us. */
    .got      : { *(.got) }      :rodata
    .dynamic  : { *(.dynamic) }  :rodata :dynamic
    __rodata_end = .;

    . = ALIGN(4K);
    __data_start = .;
    .data : {
        *(.data .data.*)
    } :data

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    } :data
    . = ALIGN(4K);
    __data_end = .;
    __kernel_end = .;

    /* No PLT: every call is direct, and a writable .got.plt would be a
     * table of code pointers an attacker could overwrite. */
    .plt     : { *(.plt .plt.*) }
    .got.plt : { *(.got.plt) }
    ASSERT(SIZEOF(.plt) == 0, "kernel must not have a PLT")
    ASSERT(SIZEOF(.got.plt) == 0, "kernel must not have a .got.plt")

    /* No unwinder in the kernel, and notes are useless at runtime. */
    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}
