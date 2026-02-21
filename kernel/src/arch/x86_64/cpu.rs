//! CPU control functions

use x86_64::instructions::{hlt, interrupts};

/// Halt CPU until next interrupt
pub fn halt() {
    hlt();
}

/// Enable interrupts
pub fn enable_interrupts() {
    interrupts::enable();
}

/// Disable interrupts  
pub fn disable_interrupts() {
    interrupts::disable();
}

/// Query interrupt flag state
pub fn interrupts_enabled() -> bool {
    interrupts::are_enabled()
}
