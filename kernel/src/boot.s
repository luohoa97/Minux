.section .bss.boot, "aw", @nobits
.align 4096
pml4_table:
    .skip 4096
pdp_table:
    .skip 4096
pd_table:
    .skip 4096
pd_table_hi3:
    .skip 4096

.align 16
stack_bottom:
    .skip 16384
stack_top:

.section .rodata.boot, "a", @progbits
.align 8
gdt64:
    .quad 0x0000000000000000
    .quad 0x00af9a000000ffff
    .quad 0x00af92000000ffff
gdt64_end:

gdt64_ptr:
    .word gdt64_end - gdt64 - 1
    .long gdt64

.section .text.boot, "ax", @progbits
.global _start
.extern kernel_main

.code32
_start:
    cli
    movl $stack_top, %esp
    movl %ebx, %eax
    movl %eax, multiboot_info_ptr

    call serial_init
    movl $boot_msg, %esi
    call serial_write

    call setup_page_tables
    call enable_long_mode

    lgdt gdt64_ptr
    ljmp $0x08, $long_mode_start

hang32:
    cli
1:  hlt
    jmp 1b

setup_page_tables:
    movl $pml4_table, %edi
    xorl %eax, %eax
    movl $4096, %ecx
    rep stosl

    movl $pdp_table, %edi
    xorl %eax, %eax
    movl $4096, %ecx
    rep stosl

    movl $pd_table, %edi
    xorl %eax, %eax
    movl $4096, %ecx
    rep stosl

    movl $pd_table_hi3, %edi
    xorl %eax, %eax
    movl $4096, %ecx
    rep stosl

    movl $pdp_table, %eax
    orl $0x3, %eax
    movl %eax, pml4_table
    movl $0, pml4_table + 4

    movl $pd_table, %eax
    orl $0x3, %eax
    movl %eax, pdp_table
    movl $0, pdp_table + 4

    // Map 3..4GiB region for APIC MMIO (IOAPIC/LAPIC)
    movl $pd_table_hi3, %eax
    orl $0x3, %eax
    movl %eax, pdp_table + (3 * 8)
    movl $0, pdp_table + (3 * 8) + 4

    xorl %ecx, %ecx
map_2m_loop:
    movl %ecx, %eax
    shll $21, %eax
    orl $0x83, %eax
    movl %eax, pd_table(,%ecx,8)
    movl $0, pd_table + 4(,%ecx,8)
    incl %ecx
    cmpl $512, %ecx
    jl map_2m_loop

    // IOAPIC @ 0xFEC00000
    movl $0xFEC00083, pd_table_hi3 + (502 * 8)
    movl $0, pd_table_hi3 + (502 * 8) + 4
    // LAPIC  @ 0xFEE00000
    movl $0xFEE00083, pd_table_hi3 + (503 * 8)
    movl $0, pd_table_hi3 + (503 * 8) + 4
    ret

enable_long_mode:
    // Verify long mode support: CPUID.80000001h:EDX.LM[29]
    movl $0x80000000, %eax
    cpuid
    cmpl $0x80000001, %eax
    jb no_long_mode
    movl $0x80000001, %eax
    cpuid
    btl $29, %edx
    jnc no_long_mode

    movl %cr4, %eax
    orl $0x20, %eax
    movl %eax, %cr4

    movl $pml4_table, %eax
    movl %eax, %cr3

    movl $0xC0000080, %ecx
    rdmsr
    orl $0x100, %eax
    wrmsr

    movl %cr0, %eax
    orl $0x80000000, %eax
    movl %eax, %cr0
    ret

no_long_mode:
    cli
1:  hlt
    jmp 1b

serial_init:
    movw $0x3F8, %dx
    movb $0x00, %al
    outb %al, %dx
    movw $0x3F9, %dx
    outb %al, %dx
    movw $0x3FB, %dx
    movb $0x80, %al
    outb %al, %dx
    movw $0x3F8, %dx
    movb $0x03, %al
    outb %al, %dx
    movw $0x3F9, %dx
    movb $0x00, %al
    outb %al, %dx
    movw $0x3FB, %dx
    movb $0x03, %al
    outb %al, %dx
    movw $0x3FA, %dx
    movb $0xC7, %al
    outb %al, %dx
    movw $0x3FC, %dx
    movb $0x0B, %al
    outb %al, %dx
    ret

serial_write:
    lodsb
    testb %al, %al
    jz serial_done
wait_tx:
    movw $0x3FD, %dx
    inb %dx, %al
    testb $0x20, %al
    jz wait_tx
    movw $0x3F8, %dx
    movb -1(%esi), %al
    outb %al, %dx
    jmp serial_write
serial_done:
    ret

.code64
long_mode_start:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movw %ax, %fs
    movw %ax, %gs

    leaq stack_top(%rip), %rsp
    xorq %rbp, %rbp

    movl multiboot_info_ptr(%rip), %edi
    call kernel_main

hang64:
    cli
2:  hlt
    jmp 2b

.section .data.boot, "aw", @progbits
multiboot_info_ptr:
    .long 0

.section .rodata.boot, "a", @progbits
boot_msg:
    .asciz "[BOOT] Entered _start, switching to long mode...\n"
