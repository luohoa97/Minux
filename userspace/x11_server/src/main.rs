//! X11-style display server skeleton.
//! Userspace-only: owns display protocol/session state and routes rendering to gfx service.

#![no_std]
#![no_main]

use libminux::{syscall, FramebufferInfo, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_GFX_ID: TaskId = 3;
const FALLBACK_INPUT_ID: TaskId = 2;
const CURSOR_SIZE: u16 = 8;

struct XServer {
    gfx_id: TaskId,
    input_id: TaskId,
    fb: Option<FramebufferInfo>,
    cursor_x: u32,
    cursor_y: u32,
    last_x: u32,
    last_y: u32,
    cursor_visible: bool,
}

impl XServer {
    fn new() -> Self {
        Self {
            gfx_id: discover_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID),
            input_id: discover_service(b"lookup:input").unwrap_or(FALLBACK_INPUT_ID),
            fb: syscall::get_framebuffer_info(),
            cursor_x: 0,
            cursor_y: 0,
            last_x: 0,
            last_y: 0,
            cursor_visible: true,
        }
    }

    fn announce(&self) {
        let _ = syscall::send_message(
            INIT_TASK_ID,
            MessageType::Request,
            b"register:x11:x11_server",
        );
        let _ = syscall::send_message(self.gfx_id, MessageType::Request, b"\n[X11] server online");
        let _ = syscall::send_message(self.gfx_id, MessageType::Request, b"\n[X11] protocol: x11:* userspace-only");
        let _ = syscall::send_message(self.input_id, MessageType::Request, b"x11:subscribe_input");
    }

    fn handle_message(&mut self, sender: TaskId, msg_type: MessageType, data: &[u8]) {
        match msg_type {
            MessageType::Request => {
                if data == b"x11:hello" {
                    let _ = syscall::reply_message(sender, b"X11:OK");
                    return;
                }
                if let Some((x, y)) = parse_cursor_set(data) {
                    self.cursor_x = x;
                    self.cursor_y = y;
                    let _ = syscall::reply_message(sender, b"X11:CURSOR");
                    self.flush_cursor();
                    return;
                }
                if let Some(vis) = parse_cursor_show(data) {
                    self.cursor_visible = vis;
                    let _ = syscall::reply_message(sender, b"X11:CURSOR");
                    self.flush_cursor();
                    return;
                }
                if data == b"x11:flush" {
                    self.flush_cursor();
                    let _ = syscall::reply_message(sender, b"X11:FLUSH");
                    return;
                }
                let _ = syscall::reply_message(sender, b"X11:UNSUPPORTED");
            }
            MessageType::Notification | MessageType::Interrupt => {
                // Future: route raw input events from input service to clients.
                let _ = sender;
            }
            _ => {}
        }
    }

    fn flush_cursor(&self) {
        if let Some(fb) = self.fb {
            let max_x = fb.width.saturating_sub(CURSOR_SIZE as u32);
            let max_y = fb.height.saturating_sub(CURSOR_SIZE as u32);
            let nx = core::cmp::min(self.cursor_x, max_x);
            let ny = core::cmp::min(self.cursor_y, max_y);
            // Clear old cursor
            draw_rect(self.gfx_id, self.last_x as u16, self.last_y as u16, CURSOR_SIZE, CURSOR_SIZE, (0, 0, 0));
            if self.cursor_visible {
                draw_rect(self.gfx_id, nx as u16, ny as u16, CURSOR_SIZE, CURSOR_SIZE, (255, 255, 255));
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut server = XServer::new();
    server.announce();

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];
                server.handle_message(sender, msg_type, data);
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn discover_service(query: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            return parse_u32_ascii(&reply[..len]);
        }
        syscall::yield_cpu();
    }
    None
}

fn draw_rect(gfx_id: TaskId, x: u16, y: u16, w: u16, h: u16, color: (u8, u8, u8)) {
    let bpr = ((w as usize) + 7) / 8;
    let mut row = [0xFFu8; 32];
    for ry in 0..h {
        let mut msg = [0u8; 64];
        msg[..4].copy_from_slice(b"FBDR");
        msg[4..6].copy_from_slice(&x.to_le_bytes());
        msg[6..8].copy_from_slice(&(y + ry).to_le_bytes());
        msg[8] = w as u8;
        msg[9] = 1;
        msg[10] = color.0;
        msg[11] = color.1;
        msg[12] = color.2;
        msg[13] = 0;
        msg[14] = 0;
        msg[15] = 0;
        let n = core::cmp::min(bpr, row.len());
        msg[16..16 + n].copy_from_slice(&row[..n]);
        send_gfx(gfx_id, &msg[..16 + bpr]);
    }
}

fn send_gfx(gfx_id: TaskId, data: &[u8]) {
    for _ in 0..64 {
        if syscall::send_message(gfx_id, MessageType::Notification, data).is_ok() {
            return;
        }
        syscall::yield_cpu();
    }
}

fn parse_cursor_set(data: &[u8]) -> Option<(u32, u32)> {
    // x11:set_cursor:<x>:<y>
    const P: &[u8] = b"x11:set_cursor:";
    if data.len() < P.len() || &data[..P.len()] != P {
        return None;
    }
    let rest = &data[P.len()..];
    let sep = rest.iter().position(|&b| b == b':')?;
    let x = parse_u32_ascii(&rest[..sep])?;
    let y = parse_u32_ascii(&rest[sep + 1..])?;
    Some((x, y))
}

fn parse_cursor_show(data: &[u8]) -> Option<bool> {
    // x11:show_cursor:0|1
    const P: &[u8] = b"x11:show_cursor:";
    if data.len() < P.len() || &data[..P.len()] != P {
        return None;
    }
    match &data[P.len()..] {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

fn parse_u32_ascii(data: &[u8]) -> Option<u32> {
    if data.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &b in data {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?;
        v = v.checked_add((b - b'0') as u32)?;
    }
    Some(v)
}

fn write_ascii(out: &mut [u8], s: &[u8]) -> usize {
    let n = core::cmp::min(out.len(), s.len());
    out[..n].copy_from_slice(&s[..n]);
    n
}

fn write_u32_ascii(id: u32, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    if id == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut n = id;
    let mut k = 0usize;
    while n > 0 && k < tmp.len() {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
    }
    let len = core::cmp::min(k, out.len());
    for j in 0..len {
        out[j] = tmp[k - 1 - j];
    }
    len
}
