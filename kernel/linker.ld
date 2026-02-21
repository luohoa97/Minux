ENTRY(_start)

SECTIONS
{
    /* Load kernel at 1MB physical address */
    . = 0x100000;

    /* Multiboot header must be in first 32KB */
    .boot :
    {
        __multiboot_header_start = .;
        KEEP(*(.multiboot_header))
        __multiboot_header_end = .;
    }

    .text : ALIGN(4K)
    {
        *(.text.boot)
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.note*)
    }

}
