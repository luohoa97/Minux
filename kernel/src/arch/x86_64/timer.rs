//! Timer interrupt handling

use core::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use x86_64::structures::idt::InterruptStackFrame;

static TICKS: AtomicU64 = AtomicU64::new(0);
const PREEMPT_QUANTUM_TICKS: u64 = 4;
const KBD_QUEUE_SIZE: usize = 64;
static KBD_QUEUE: [AtomicU8; KBD_QUEUE_SIZE] = [const { AtomicU8::new(0) }; KBD_QUEUE_SIZE];
static KBD_HEAD: AtomicUsize = AtomicUsize::new(0);
static KBD_TAIL: AtomicUsize = AtomicUsize::new(0);

/// Program PIT channel 0 in rate generator mode.
pub fn init_pit(hz: u32) {
    // PIT input clock: 1193182 Hz.
    let divider = if hz == 0 { 11931 } else { (1_193_182u32 / hz).max(1) as u16 };
    unsafe {
        outb(0x43, 0x36); // ch0, lobyte/hibyte, mode 3, binary
        outb(0x40, (divider & 0xff) as u8);
        outb(0x40, (divider >> 8) as u8);
    }
    crate::serial_println!("[TIMER] PIT initialized at {} Hz", hz);
}

/// Timer interrupt handler
pub extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if (ticks % 1000) == 0 {
        crate::serial_debugln!("[DBG] timer tick={}", ticks);
    }
    if (ticks % PREEMPT_QUANTUM_TICKS) == 0 {
        // Keep timer-driven preemption disabled during bring-up.
        // `schedule()` performs a task-context switch and currently expects to run
        // from normal kernel control flow, not from an interrupt trapframe.
        // Calling it here can save IRQ-frame RIP/RSP into task state and corrupt
        // resumed user execution.
    }

    // Acknowledge interrupt.
    unsafe {
        super::pic::PICS.lock()
            .notify_end_of_interrupt(super::pic::InterruptIndex::Timer.as_u8());
    }
}

/// Keyboard IRQ1 handler (legacy PS/2).
pub extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    let scancode = unsafe { inb(0x60) };
    push_keyboard_scancode(scancode);
    unsafe {
        super::pic::PICS.lock()
            .notify_end_of_interrupt(super::pic::InterruptIndex::Keyboard.as_u8());
    }
}

/// Serial COM1 IRQ4 handler.
pub extern "x86-interrupt" fn serial_com1_handler(_stack_frame: InterruptStackFrame) {
    let _iir = unsafe { inb(0x3f8 + 2) }; // acknowledge by reading IIR
    crate::serial_debugln!("[IRQ] serial COM1");
    unsafe {
        super::pic::PICS.lock()
            .notify_end_of_interrupt(super::pic::InterruptIndex::SerialCom1.as_u8());
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

#[inline]
fn push_keyboard_scancode(sc: u8) {
    let head = KBD_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % KBD_QUEUE_SIZE;
    let tail = KBD_TAIL.load(Ordering::Acquire);
    if next == tail {
        return;
    }
    KBD_QUEUE[head].store(sc, Ordering::Relaxed);
    KBD_HEAD.store(next, Ordering::Release);
}

pub fn poll_keyboard_scancode() -> Option<u8> {
    let tail = KBD_TAIL.load(Ordering::Relaxed);
    let head = KBD_HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let sc = KBD_QUEUE[tail].load(Ordering::Relaxed);
    KBD_TAIL.store((tail + 1) % KBD_QUEUE_SIZE, Ordering::Release);
    Some(sc)
}
