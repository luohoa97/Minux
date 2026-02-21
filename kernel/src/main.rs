#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::arch::global_asm;

mod microkernel;
mod arch;
mod ipc;
mod mm;
mod serial; // Early boot serial output (before user-space drivers)

use core::panic::PanicInfo;

global_asm!(include_str!("boot.s"), options(att_syntax));

// Force multiboot header to be linked
#[used]
static MULTIBOOT_FORCE_LINK: extern "C" fn() = arch::x86_64::boot::multiboot_header::__multiboot_header_marker;

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(multiboot_info: u32) -> ! {
    let multiboot_info = multiboot_info as usize;
    // Early boot serial output (before user-space drivers)
    serial::init();
    serial_println!("[KERNEL] Minux L4 microkernel starting...");
    
    serial_println!("[KERNEL] Initializing CPU architecture...");
    arch::init();
    
    serial_println!("[KERNEL] Initializing address spaces...");
    mm::init();
    
    serial_println!("[KERNEL] Initializing IPC...");
    ipc::init();
    
    serial_println!("[KERNEL] Initializing threads and scheduler...");
    microkernel::init();
    
    serial_println!("[KERNEL] Loading boot modules to user-space...");
    serial_println!("[KERNEL] Multiboot info at: 0x{:x}", multiboot_info);
    microkernel::load_boot_modules(multiboot_info);
    
    serial_println!("[KERNEL] Enabling interrupts...");
    arch::enable_interrupts();

    serial_println!("[KERNEL] Entering microkernel main loop...");
    serial_println!("[KERNEL] Serial output will now be handled by user-space driver");
    
    // Enter microkernel main loop (schedule threads, route IPC)
    microkernel::run();
}

/// Secondary CPU entry point (called by AP trampoline after long-mode handoff).
#[unsafe(no_mangle)]
pub extern "C" fn ap_kernel_main(apic_id: u32) -> ! {
    serial::init();
    serial_println!("[SMP] AP {} entered ap_kernel_main", apic_id);
    arch::x86_64::mark_ap_online(apic_id);

    // APs run the same kernel loop with per-core scheduler state.
    microkernel::run();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("[KERNEL PANIC] {}", info);
    kernel_fatal("Kernel panic (see serial for details)");
}

/// Enter fatal kernel state: show a visible message and halt forever.
pub fn kernel_fatal(message: &str) -> ! {
    serial_println!("[KERNEL FATAL] {}", message);

    arch::disable_interrupts();
    loop {
        arch::halt();
    }
}
