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
 *   rodata    R--   constants, strings, font
 *   data      RW-   .data + .bss (NX)
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
    dynamic  PT_DYNAMIC FLAGS(4);
    data     PT_LOAD FLAGS(6);       /* R+W */
}

SECTIONS
{
    /* Nominal higher-half base. Limine chooses the runtime base. */
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
    } :rodata
    /* Limine processes these relocations before applying page permissions.
       Keep the metadata and relocated read-only data inside our R segment. */
    .dynsym : { *(.dynsym) } :rodata
    .dynstr : { *(.dynstr) } :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .hash : { *(.hash) } :rodata
    .rela.dyn : { *(.rela.dyn) } :rodata
    .dynamic : { *(.dynamic) } :rodata :dynamic
    .got : { *(.got .got.*) } :rodata
    .data.rel.ro : { *(.data.rel.ro .data.rel.ro.*) } :rodata
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

    /* No unwinder in the kernel, and notes are useless at runtime. */
    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}
