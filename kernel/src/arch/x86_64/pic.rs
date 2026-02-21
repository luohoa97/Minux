//! Programmable Interrupt Controller management

use pic8259::ChainedPics;
use spin::Mutex;

/// PIC configuration
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Chained PICs for handling hardware interrupts
pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Interrupt indices
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard = PIC_1_OFFSET + 1,
    SerialCom1 = PIC_1_OFFSET + 4,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

/// Initialize PICs
pub fn init() {
    unsafe {
        PICS.lock().initialize();
    }
    // Unmask IRQ0 (timer), IRQ1 (keyboard), IRQ4 (COM1 serial).
    unsafe {
        let mut mask1 = inb(0x21);
        mask1 &= !(1 << 0);
        mask1 &= !(1 << 1);
        mask1 &= !(1 << 4);
        outb(0x21, mask1);
    }
}

#[inline]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}
