//! VGA text mode driver for Minux
//! 
//! This is a userspace driver that manages VGA text output.
//! Other programs send messages to this driver to display text.

#![no_std]
#![no_main]

use libminux::{syscall, vga, MessageType, TaskId};
const INIT_TASK_ID: TaskId = 2;

/// VGA driver state
struct VgaDriver {
    cursor_x: usize,
    cursor_y: usize,
    line_len: [u16; vga::VGA_HEIGHT],
}

impl VgaDriver {
    fn new() -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
            line_len: [0; vga::VGA_HEIGHT],
        }
    }
    
    /// Initialize VGA driver
    fn init(&mut self) {
        unsafe {
            // Clear screen with black background
            vga::clear_screen(vga::Color::Black);
            
            // Display startup message
            vga::write_string(0, 0, "Minux VGA Driver v1.0", vga::Color::White, vga::Color::Black);
            vga::write_string(0, 1, "Ready to serve display requests", vga::Color::Green, vga::Color::Black);
        }
        
        self.cursor_x = 0;
        self.cursor_y = 3; // Start below header
        self.line_len = [0; vga::VGA_HEIGHT];
    }
    
    /// Handle incoming message
    fn handle_message(&mut self, sender: TaskId, msg_type: MessageType, data: &[u8]) {
        match msg_type {
            MessageType::Request => {
                // Display text request
                if let Ok(text) = core::str::from_utf8(data) {
                    self.display_text(text);
                    
                    // Send acknowledgment
                    let _ = syscall::reply_message(sender, b"OK");
                }
            }
            MessageType::Notification => {
                // Handle notifications (e.g., clear screen)
                if data == b"clear" {
                    unsafe {
                        vga::clear_screen(vga::Color::Black);
                    }
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                    self.line_len = [0; vga::VGA_HEIGHT];
                }
            }
            _ => {
                // Ignore other message types
            }
        }
    }
    
    /// Display text on screen
    fn display_text(&mut self, text: &str) {
        unsafe {
            for ch in text.bytes() {
                match ch {
                    b'\n' => {
                        if self.cursor_x > self.line_len[self.cursor_y] as usize {
                            self.line_len[self.cursor_y] = self.cursor_x as u16;
                        }
                        self.cursor_x = 0;
                        self.cursor_y += 1;
                        if self.cursor_y >= vga::VGA_HEIGHT {
                            self.scroll_up();
                        }
                    }
                    b'\r' => {
                        self.cursor_x = 0;
                    }
                    0x08 => {
                        if self.cursor_x > 0 {
                            self.cursor_x -= 1;
                        } else if self.cursor_y > 0 {
                            self.cursor_y -= 1;
                            self.cursor_x = self.line_len[self.cursor_y] as usize;
                            if self.cursor_x > 0 {
                                self.cursor_x -= 1;
                            }
                        } else {
                            continue;
                        }
                        vga::write_char(
                            self.cursor_x,
                            self.cursor_y,
                            b' ',
                            vga::Color::White,
                            vga::Color::Black,
                        );
                        if self.cursor_x < self.line_len[self.cursor_y] as usize {
                            self.line_len[self.cursor_y] = self.cursor_x as u16;
                        }
                    }
                    ch => {
                        if self.cursor_x >= vga::VGA_WIDTH {
                            self.line_len[self.cursor_y] = vga::VGA_WIDTH as u16;
                            self.cursor_x = 0;
                            self.cursor_y += 1;
                            if self.cursor_y >= vga::VGA_HEIGHT {
                                self.scroll_up();
                            }
                        }
                        
                        vga::write_char(
                            self.cursor_x,
                            self.cursor_y,
                            ch,
                            vga::Color::White,
                            vga::Color::Black,
                        );
                        
                        self.cursor_x += 1;
                        if self.cursor_x > self.line_len[self.cursor_y] as usize {
                            self.line_len[self.cursor_y] = self.cursor_x as u16;
                        }
                    }
                }
            }
        }
    }
    
    /// Scroll screen up by one line
    fn scroll_up(&mut self) {
        unsafe {
            // Move all lines up
            for y in 1..vga::VGA_HEIGHT {
                for x in 0..vga::VGA_WIDTH {
                    let src_offset = (y * vga::VGA_WIDTH + x) * 2;
                    let dst_offset = ((y - 1) * vga::VGA_WIDTH + x) * 2;
                    
                    let ch = *vga::VGA_BUFFER.add(src_offset);
                    let color = *vga::VGA_BUFFER.add(src_offset + 1);
                    
                    *vga::VGA_BUFFER.add(dst_offset) = ch;
                    *vga::VGA_BUFFER.add(dst_offset + 1) = color;
                }
            }
            
            // Clear bottom line
            for x in 0..vga::VGA_WIDTH {
                vga::write_char(x, vga::VGA_HEIGHT - 1, b' ', vga::Color::White, vga::Color::Black);
            }
        }
        for y in 1..vga::VGA_HEIGHT {
            self.line_len[y - 1] = self.line_len[y];
        }
        self.line_len[vga::VGA_HEIGHT - 1] = 0;
        
        self.cursor_y = vga::VGA_HEIGHT - 1;
    }
}

/// Entry point for VGA driver
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut driver = VgaDriver::new();
    driver.init();
    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:display:vga_driver",
    );
    
    // Main driver loop - wait for messages
    loop {
        let mut buf = [0u8; 256];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                driver.handle_message(sender, msg_type, &buf[..len]);
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

// Panic handler is provided by libminux
