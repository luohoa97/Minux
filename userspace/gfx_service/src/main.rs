//! Graphics service.
//! App-facing API; forwards draw requests to display driver.

#![no_std]
#![no_main]

use libminux::{syscall, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_DISPLAY_ID: TaskId = 3;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let display_id = wait_for_display_service().unwrap_or(FALLBACK_DISPLAY_ID);
    let _ = syscall::send_message(display_id, MessageType::Request, b"\n[GFX] service online");
    let _ = syscall::send_message(
        INIT_TASK_ID,
        MessageType::Request,
        b"register:gfx:gfx_service",
    );

    loop {
        let mut buf = [0u8; 64];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = frame_len(&buf);
                let data = &buf[..len];
                match msg_type {
                    MessageType::Request => {
                        let _ = syscall::send_message(display_id, MessageType::Notification, data);
                        let _ = syscall::reply_message(sender, b"GFX:OK");
                    }
                    MessageType::Notification => {
                        let _ = syscall::send_message(display_id, MessageType::Notification, data);
                    }
                    // Ignore reply/interrupt frames not part of gfx API.
                    _ => {}
                }
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

fn discover_display_service() -> Option<TaskId> {
    // Prefer framebuffer driver if present; fall back to VGA text display.
    if let Some(id) = discover_service_class(b"lookup:display_vesa") {
        return Some(id);
    }
    discover_service_class(b"lookup:display")
}

fn discover_service_class(query: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    let mut reply = [0u8; 16];
    if let Ok((sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        if sender != INIT_TASK_ID {
            return None;
        }
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        let id = parse_u32_ascii(&reply[..len])?;
        if id == INIT_TASK_ID {
            return None;
        }
        return Some(id);
    }
    None
}

fn wait_for_display_service() -> Option<TaskId> {
    for _ in 0..1_000_000u32 {
        if let Some(id) = discover_display_service() {
            return Some(id);
        }
        syscall::yield_cpu();
    }
    None
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
    if v == 0 { None } else { Some(v) }
}
