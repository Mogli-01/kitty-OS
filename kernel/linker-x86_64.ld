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
    data     PT_LOAD FLAGS(6);       /* R+W */
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
    } :rodata
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
