//! x86_64 architecture support for minux microkernel

mod cpu;
mod pic;
mod gdt;
mod idt;
mod exceptions;
mod interrupts;
mod timer;
mod syscall;
mod smp;
mod lapic;
mod fpu;
pub mod boot;
use spin::Mutex;

pub use cpu::*;
pub use boot::{BootModule, BootProtocol, FramebufferInfo, detect_boot_protocol, get_boot_modules};
// Bring-up mode: user tasks still run at CPL0 on task stacks, so hardware IRQ
// entry can corrupt task control flow without a dedicated ring-transition stack.
const ENABLE_HW_INTERRUPTS: bool = true;
static BOOT_FRAMEBUFFER: Mutex<Option<FramebufferInfo>> = Mutex::new(None);

/// Initialize x86_64 architecture
pub fn init() {
    fpu::init();
    gdt::init();
    smp::init();
    interrupts::init();
}

pub fn send_reschedule_ipi(dest_apic_id: u32) {
    lapic::send_ipi(dest_apic_id, 0xF0, 0, true, false);
}

pub fn mark_ap_online(apic_id: u32) {
    smp::mark_ap_online(apic_id);
}

/// Enable interrupts
pub fn enable_interrupts() {
    if ENABLE_HW_INTERRUPTS {
        cpu::enable_interrupts();
        crate::serial_debugln!("[DBG] CPU interrupts enabled");
    } else {
        crate::serial_debugln!("[DBG] CPU interrupts remain disabled (bring-up mode)");
    }
}

/// Disable interrupts
pub fn disable_interrupts() {
    cpu::disable_interrupts();
}

/// Halt CPU until next interrupt
pub fn halt() {
    cpu::halt();
}

pub fn interrupts_enabled() -> bool {
    cpu::interrupts_enabled()
}

pub fn poll_keyboard_scancode() -> Option<u8> {
    timer::poll_keyboard_scancode()
}

pub unsafe fn fpu_save(area_16b_aligned: *mut u8) {
    fpu::fpu_save(area_16b_aligned);
}

pub unsafe fn fpu_restore(area_16b_aligned: *const u8) {
    fpu::fpu_restore(area_16b_aligned);
}

pub fn fpu_reset_thread() {
    fpu::fpu_reset_thread();
}

/// Switch to different address space
pub fn switch_address_space(address_space_id: u32) {
    if let Some(address_space) = crate::mm::get_address_space(address_space_id) {
        unsafe {
            // Load CR3 register with new page table root
            core::arch::asm!("mov cr3, {}", in(reg) address_space.page_table_root);
            
            // Flush TLB to ensure address translation is updated
            core::arch::asm!("mov {tmp}, cr3; mov cr3, {tmp}", tmp = out(reg) _);
        }
    }
}

/// Transfer execution to a task entry point with its stack.
/// This is the first non-stub user-task handoff for bring-up.
pub fn enter_task(entry: u64, stack: u64) -> ! {
    unsafe {
        core::arch::asm!(
            "mov rsp, {stack}",
            "xor rbp, rbp",
            "jmp {entry}",
            stack = in(reg) stack,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
}

pub fn set_boot_framebuffer(info: Option<FramebufferInfo>) {
    *BOOT_FRAMEBUFFER.lock() = info;
}

pub fn boot_framebuffer() -> Option<FramebufferInfo> {
    *BOOT_FRAMEBUFFER.lock()
}
