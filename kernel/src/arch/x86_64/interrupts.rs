//! Interrupt subsystem coordination

/// Initialize interrupt subsystem
pub fn init() {
    crate::serial_debugln!("[DBG] interrupts::init: PIC init");
    super::pic::init();
    crate::serial_debugln!("[DBG] interrupts::init: PIC init done");

    crate::serial_debugln!("[DBG] interrupts::init: IDT init");
    super::idt::init();
    crate::serial_debugln!("[DBG] interrupts::init: IDT init done");

    super::timer::init_pit(1000);
}
