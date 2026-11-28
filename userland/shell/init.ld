ENTRY(_start)
PHDRS {
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
}
SECTIONS {
    . = 0x400000;
    .text : { *(.text .text.*) *(.rodata .rodata.*) } :text
    . = ALIGN(4096);
    .data : { BYTE(0); *(.data .data.*) *(.got .got.*) } :data
    .bss : { *(.bss .bss.*) *(COMMON) } :data
    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.comment) *(.note .note.*) }
}
