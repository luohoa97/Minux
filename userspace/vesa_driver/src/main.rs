//! VESA driver placeholder service.
//! For now this uses VGA text-mode as backend while keeping a VESA-facing service name.

#![no_std]
#![no_main]

use libminux::{syscall, vga, MessageType, TaskId};
const INIT_TASK_ID: TaskId = 2;

struct VesaDriver {
    cursor_x: usize,
    cursor_y: usize,
}

impl VesaDriver {
    fn new() -> Self {
        Self { cursor_x: 0, cursor_y: 0 }
    }

    fn init(&mut self) {
        // Own device memory setup in userspace; kernel remains driver-agnostic.
        let _ = syscall::map_page(0xb8000, 0xb8000, 0x3 | 0x8);
        unsafe {
            vga::clear_screen(vga::Color::Black);
            vga::write_string(0, 0, "Minux VESA Driver (text backend)", vga::Color::White, vga::Color::Black);
            vga::write_string(0, 1, "Protocol ready", vga::Color::Green, vga::Color::Black);
        }
        self.cursor_x = 0;
        self.cursor_y = 3;
    }

    fn handle_message(&mut self, sender: TaskId, msg_type: MessageType, data: &[u8]) {
        match msg_type {
            MessageType::Request => {
                if data.starts_with(b"clear") {
                    unsafe { vga::clear_screen(vga::Color::Black); }
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                } else if let Ok(text) = core::str::from_utf8(data) {
                    self.display_text(text);
                }
                let _ = syscall::reply_message(sender, b"OK");
            }
            MessageType::Notification => {
                if data == b"clear" {
                    unsafe { vga::clear_screen(vga::Color::Black); }
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                } else if let Ok(text) = core::str::from_utf8(data) {
                    self.display_text(text);
                }
            }
            _ => {}
        }
    }

    fn display_text(&mut self, text: &str) {
        unsafe {
            for ch in text.bytes() {
                match ch {
                    b'\n' => {
                        self.cursor_x = 0;
                        self.cursor_y += 1;
                    }
                    b'\r' => self.cursor_x = 0,
                    c => {
                        if self.cursor_x >= vga::VGA_WIDTH {
                            self.cursor_x = 0;
                            self.cursor_y += 1;
                        }
                        if self.cursor_y >= vga::VGA_HEIGHT {
                            self.cursor_y = 0;
                        }
                        vga::write_char(self.cursor_x, self.cursor_y, c, vga::Color::White, vga::Color::Black);
                        self.cursor_x += 1;
                    }
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut driver = VesaDriver::new();
    driver.init();
    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:display:vesa_driver",
    );

    loop {
        let mut buf = [0u8; 256];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];
                driver.handle_message(sender, msg_type, data);
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}
