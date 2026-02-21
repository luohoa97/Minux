//! Serial port output for early boot debugging
//! This is ONLY used during kernel initialization before user-space drivers load
//! After boot, serial should be handled by a user-space driver

use core::fmt;

const SERIAL_PORT: u16 = 0x3F8; // COM1
pub const DEBUG_LOGS: bool = true;

/// Initialize serial port for early boot output
pub fn init() {
    unsafe {
        // Disable interrupts
        outb(SERIAL_PORT + 1, 0x00);
        // Enable DLAB
        outb(SERIAL_PORT + 3, 0x80);
        // Set divisor to 3 (38400 baud)
        outb(SERIAL_PORT + 0, 0x03);
        outb(SERIAL_PORT + 1, 0x00);
        // 8 bits, no parity, one stop bit
        outb(SERIAL_PORT + 3, 0x03);
        // Enable FIFO
        outb(SERIAL_PORT + 2, 0xC7);
        // Mark data terminal ready
        outb(SERIAL_PORT + 4, 0x0B);
    }
}

/// Write byte to serial port
fn write_byte(byte: u8) {
    unsafe {
        // Wait for transmit buffer to be empty
        while (inb(SERIAL_PORT + 5) & 0x20) == 0 {}
        outb(SERIAL_PORT, byte);
    }
}

/// Write string to serial port
pub fn write_str(s: &str) {
    for byte in s.bytes() {
        write_byte(byte);
    }
}

/// Write formatted string to serial
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    SerialWriter.write_fmt(args).unwrap();
}

struct SerialWriter;

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_str(s);
        Ok(())
    }
}

/// Print macro for early boot debugging
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

/// Print with newline
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

/// Debug print with newline (compile-time toggle in `serial::DEBUG_LOGS`)
#[macro_export]
macro_rules! serial_debugln {
    () => {{
        if $crate::serial::DEBUG_LOGS {
            $crate::serial_println!();
        }
    }};
    ($($arg:tt)*) => {{
        if $crate::serial::DEBUG_LOGS {
            $crate::serial_println!($($arg)*);
        }
    }};
}

/// Port I/O functions
#[inline]
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}
