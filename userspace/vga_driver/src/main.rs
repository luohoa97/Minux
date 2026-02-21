//! VGA text mode driver for Minux
//! 
//! This is a userspace driver that manages VGA text output.
//! Other programs send messages to this driver to display text.

#![no_std]
#![no_main]

use libminux::{syscall, vga, MessageType, TaskId};

/// VGA driver state
struct VgaDriver {
    cursor_x: usize,
    cursor_y: usize,
}

impl VgaDriver {
    fn new() -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
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
                        self.cursor_x = 0;
                        self.cursor_y += 1;
                        if self.cursor_y >= vga::VGA_HEIGHT {
                            self.scroll_up();
                        }
                    }
                    b'\r' => {
                        self.cursor_x = 0;
                    }
                    ch => {
                        if self.cursor_x >= vga::VGA_WIDTH {
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
        
        self.cursor_y = vga::VGA_HEIGHT - 1;
    }
}

/// Entry point for VGA driver
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut driver = VgaDriver::new();
    driver.init();
    
    // Main driver loop - wait for messages
    loop {
        match syscall::receive_message_zc() {
            Ok((sender, msg_type, ptr, len)) => {
                let data = if !ptr.is_null() && len > 0 {
                    unsafe { core::slice::from_raw_parts(ptr, len) }
                } else {
                    &[]
                };
                driver.handle_message(sender, msg_type, data);
            }
            Err(_) => {
                // No message available, yield CPU
                syscall::yield_cpu();
            }
        }
    }
}

// Panic handler is provided by libminux
