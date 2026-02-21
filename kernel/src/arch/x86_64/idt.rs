//! Interrupt Descriptor Table management

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }
}

static mut IDT_RAW: [IdtEntry; 256] = [IdtEntry::missing(); 256];

const KERNEL_CODE_SELECTOR: u16 = 0x08;
const INTERRUPT_GATE_PRESENT: u8 = 0x8E;
const DOUBLE_FAULT_IST: u8 = 1;

unsafe fn set_gate(vector: usize, handler: u64, ist: u8) {
    let entry = IdtEntry {
        offset_low: (handler & 0xFFFF) as u16,
        selector: KERNEL_CODE_SELECTOR,
        ist: ist & 0x7,
        type_attr: INTERRUPT_GATE_PRESENT,
        offset_mid: ((handler >> 16) & 0xFFFF) as u16,
        offset_high: ((handler >> 32) & 0xFFFF_FFFF) as u32,
        zero: 0,
    };
    IDT_RAW[vector] = entry;
}

/// Initialize IDT
pub fn init() {
    crate::serial_debugln!("[DBG] idt::init enter");

    unsafe {
        set_gate(0, super::exceptions::divide_error_handler as usize as u64, 0);
        set_gate(6, super::exceptions::invalid_opcode_handler as usize as u64, 0);
        set_gate(8, super::exceptions::double_fault_handler as usize as u64, DOUBLE_FAULT_IST);
        set_gate(13, super::exceptions::general_protection_fault_handler as usize as u64, 0);
        set_gate(14, super::exceptions::page_fault_handler as usize as u64, 0);
        set_gate(
            super::pic::InterruptIndex::Timer.as_usize(),
            super::timer::timer_handler as usize as u64,
            0,
        );
        set_gate(
            super::pic::InterruptIndex::Keyboard.as_usize(),
            super::timer::keyboard_handler as usize as u64,
            0,
        );
        set_gate(
            super::pic::InterruptIndex::SerialCom1.as_usize(),
            super::timer::serial_com1_handler as usize as u64,
            0,
        );
        set_gate(0x80, super::syscall::syscall_entry_asm as usize as u64, 0);
    }
    crate::serial_debugln!("[DBG] idt::critical handlers installed");

    let idtr = Idtr {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT_RAW) as u64,
    };

    crate::serial_debugln!("[DBG] idt::load");
    unsafe {
        core::arch::asm!(
            "lidt [{0}]",
            in(reg) &idtr,
            options(readonly, nostack, preserves_flags)
        );
    }

    crate::serial_debugln!("[DBG] idt::init done");
}
