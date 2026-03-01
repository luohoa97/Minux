//! VESA linear-framebuffer driver (userspace).
//! Framebuffer primitive endpoint: clear/fill/draw mono bitmap.

#![no_std]
#![no_main]

use libminux::{syscall, FramebufferInfo, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FB_VIRT_BASE: u64 = 0x0000_6000_0000_0000;
const MAP_FLAGS: u64 = 0x1 | 0x2 | 0x8; // R|W|USER

struct VesaDriver {
    fb: FramebufferInfo,
    base: *mut u8,
}

impl VesaDriver {
    fn new(fb: FramebufferInfo) -> Self {
        Self {
            fb,
            base: FB_VIRT_BASE as *mut u8,
        }
    }

    fn map_framebuffer(&self) -> bool {
        let bytes = (self.fb.pitch as u64).saturating_mul(self.fb.height as u64);
        if bytes == 0 {
            return false;
        }
        let pages = ((bytes + 0xfff) / 0x1000) as usize;
        let phys_base = self.fb.phys_addr & !0xfff;
        let page_off = self.fb.phys_addr & 0xfff;
        let virt_base = FB_VIRT_BASE.saturating_add(page_off);
        for i in 0..pages {
            let va = virt_base + (i as u64) * 0x1000;
            let pa = phys_base + (i as u64) * 0x1000;
            if syscall::map_page(va, pa, MAP_FLAGS).is_err() {
                return false;
            }
        }
        true
    }

    fn clear_rgb(&mut self, r: u8, g: u8, b: u8) {
        if self.fb.bpp != 32 {
            return;
        }
        let h = self.fb.height as usize;
        let w = self.fb.width as usize;
        let pitch = self.fb.pitch as usize;
        unsafe {
            for y in 0..h {
                let row = self.base.add(y * pitch);
                for x in 0..w {
                    let p = row.add(x * 4);
                    core::ptr::write_volatile(p.add(0), b);
                    core::ptr::write_volatile(p.add(1), g);
                    core::ptr::write_volatile(p.add(2), r);
                    core::ptr::write_volatile(p.add(3), 0);
                }
            }
        }
    }

    fn put_pixel(&self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if self.fb.bpp != 32 || x >= self.fb.width as usize || y >= self.fb.height as usize {
            return;
        }
        let pitch = self.fb.pitch as usize;
        unsafe {
            let p = self.base.add(y * pitch + x * 4);
            core::ptr::write_volatile(p.add(0), b);
            core::ptr::write_volatile(p.add(1), g);
            core::ptr::write_volatile(p.add(2), r);
            core::ptr::write_volatile(p.add(3), 0);
        }
    }

    fn handle(&mut self, sender: TaskId, msg_type: MessageType, data: &[u8]) {
        if data == b"clear" || data == b"FBCL" {
            self.clear_rgb(0, 0, 0);
            if matches!(msg_type, MessageType::Request) {
                let _ = syscall::reply_message(sender, b"OK");
            }
            return;
        }
        if let Some((r, g, b)) = parse_fill(data) {
            self.clear_rgb(r, g, b);
            if matches!(msg_type, MessageType::Request) {
                let _ = syscall::reply_message(sender, b"OK");
            }
            return;
        }
        if data.len() >= 16 && &data[..4] == b"FBDR" {
            let x = u16::from_le_bytes([data[4], data[5]]) as usize;
            let y = u16::from_le_bytes([data[6], data[7]]) as usize;
            let w = data[8] as usize;
            let h = data[9] as usize;
            let fg = (data[10], data[11], data[12]);
            let bg = (data[13], data[14], data[15]);
            let bpr = (w + 7) / 8;
            let need = 16 + bpr * h;
            if need <= data.len() {
                let bits = &data[16..need];
                for ry in 0..h {
                    for rx in 0..w {
                        let byte = bits[ry * bpr + (rx / 8)];
                        let bit = 0x80 >> (rx % 8);
                        let (r, g, b) = if (byte & bit) != 0 { fg } else { bg };
                        self.put_pixel(x + rx, y + ry, r, g, b);
                    }
                }
            }
            if matches!(msg_type, MessageType::Request) {
                let _ = syscall::reply_message(sender, b"OK");
            }
            return;
        }
        if matches!(msg_type, MessageType::Request) {
            let _ = syscall::reply_message(sender, b"OK");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let fb = match syscall::get_framebuffer_info() {
        Some(v) => v,
        None => loop {
            syscall::yield_cpu();
        },
    };
    let mut driver = VesaDriver::new(fb);
    if driver.map_framebuffer() {
        driver.clear_rgb(0, 0, 0);
    }

    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:display_vesa:vesa_driver",
    );

    loop {
        let mut buf = [0u8; 64];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = frame_len(&buf);
                driver.handle(sender, msg_type, &buf[..len]);
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn frame_len(buf: &[u8]) -> usize {
    if buf.len() >= 16 && &buf[..4] == b"FBDR" {
        let w = buf[8] as usize;
        let h = buf[9] as usize;
        let bpr = (w + 7) / 8;
        return core::cmp::min(16 + bpr * h, buf.len());
    }
    if buf.len() >= 12 && &buf[..6] == b"fill:#" {
        return 12;
    }
    if buf.len() >= 4 && &buf[..4] == b"FBCL" {
        return 4;
    }
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

fn parse_fill(data: &[u8]) -> Option<(u8, u8, u8)> {
    const P: &[u8] = b"fill:#";
    if data.len() != 12 || &data[..P.len()] != P {
        return None;
    }
    let r = parse_hex_u8(&data[6..8])?;
    let g = parse_hex_u8(&data[8..10])?;
    let b = parse_hex_u8(&data[10..12])?;
    Some((r, g, b))
}

fn parse_hex_u8(data: &[u8]) -> Option<u8> {
    if data.len() != 2 {
        return None;
    }
    let hi = hex_nibble(data[0])?;
    let lo = hex_nibble(data[1])?;
    Some((hi << 4) | lo)
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
