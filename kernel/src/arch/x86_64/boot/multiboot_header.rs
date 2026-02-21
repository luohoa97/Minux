//! Multiboot2 header embedded in the kernel binary
//! This must be in the first 32KB of the kernel

use core::arch::global_asm;

// Embed multiboot2 header in assembly
global_asm!(
    ".section .multiboot_header,\"a\",@progbits",
    ".align 8",
    "multiboot_header_start:",
    "    .long 0xe85250d6",                // magic
    "    .long 0",                          // architecture (i386)
    "    .long multiboot_header_end - multiboot_header_start",  // header length
    "    .long -(0xe85250d6 + 0 + (multiboot_header_end - multiboot_header_start))", // checksum
    "",
    "    // End tag",
    "    .short 0",    // type
    "    .short 0",    // flags  
    "    .long 8",     // size
    "multiboot_header_end:",
);

#[unsafe(no_mangle)]
pub extern "C" fn __multiboot_header_marker() {}
