//! Architecture abstraction layer for minux microkernel
//! 
//! This module provides a hardware abstraction layer that isolates
//! the microkernel core from architecture-specific details.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use self::x86_64 as current;

/// Architecture-independent interface
pub trait ArchInterface {
    /// Initialize architecture-specific components
    fn init();
    
    /// Enable interrupts
    fn enable_interrupts();
    
    /// Disable interrupts
    fn disable_interrupts();
    
    /// Halt CPU until next interrupt
    fn halt();
}

/// Initialize architecture layer
pub fn init() {
    current::init();
}

/// Enable interrupts
pub fn enable_interrupts() {
    current::enable_interrupts();
}

/// Disable interrupts  
pub fn disable_interrupts() {
    current::disable_interrupts();
}

/// Halt CPU until next interrupt
pub fn halt() {
    current::halt();
}

/// Returns true when IF is set on this CPU.
pub fn interrupts_enabled() -> bool {
    current::interrupts_enabled()
}

/// Switch to different address space
pub fn switch_address_space(address_space_id: u32) {
    current::switch_address_space(address_space_id);
}

/// Get boot modules from bootloader
pub fn get_boot_modules(boot_info: usize, protocol: current::BootProtocol) -> &'static [current::BootModule] {
    current::get_boot_modules(boot_info, protocol)
}

/// Detect boot protocol
pub fn detect_boot_protocol(boot_info: usize) -> current::BootProtocol {
    current::detect_boot_protocol(boot_info)
}

/// Send a reschedule IPI to a target APIC ID (x86_64 only).
#[cfg(target_arch = "x86_64")]
pub fn send_reschedule_ipi(dest_apic_id: u32) {
    current::send_reschedule_ipi(dest_apic_id);
}
