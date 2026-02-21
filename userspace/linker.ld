ENTRY(_start)

SECTIONS
{
    /* Keep userspace image away from kernel low-half mappings. */
    . = 0x40000000;

    .text : ALIGN(4K)
    {
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
