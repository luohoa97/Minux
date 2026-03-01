//! Snake demo process for Minux.
//! Uses gfx service to render a simple auto-moving snake frame with pixels.

#![no_std]
#![no_main]

use libminux::{syscall, FramebufferInfo, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_GFX_ID: u32 = 3;
const CELL: usize = 8;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let gfx_id = discover_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let fb = syscall::get_framebuffer_info();
    let (cols, rows) = grid_size(fb);
    if cols < 4 || rows < 4 {
        loop {
            syscall::yield_cpu();
        }
    }
    send_gfx(gfx_id, b"FBCL");

    let mut x = 2usize;
    let mut y = 2usize;
    let mut dx: isize = 1;
    let mut dy: isize = 0;
    let mut tick: u64 = 0;

    loop {
        tick += 1;
        if tick % 2_000_000 != 0 {
            syscall::yield_cpu();
            continue;
        }

        if x + 1 >= cols {
            dx = 0;
            dy = 1;
        } else if y + 1 >= rows {
            dx = -1;
            dy = 0;
        } else if x == 0 {
            dx = 0;
            dy = -1;
        } else if y == 0 {
            dx = 1;
            dy = 0;
        }

        x = ((x as isize) + dx) as usize;
        y = ((y as isize) + dy) as usize;

        render(gfx_id, cols, rows, x, y);
        syscall::yield_cpu();
    }
}

fn render(gfx_id: u32, cols: usize, rows: usize, sx: usize, sy: usize) {
    send_gfx(gfx_id, b"FBCL");
    for y in 0..rows {
        for x in 0..cols {
            let border = x == 0 || y == 0 || x + 1 == cols || y + 1 == rows;
            let color = if border { (200, 200, 200) } else { (0, 0, 0) };
            draw_cell(gfx_id, x, y, color, (0, 0, 0));
        }
    }
    draw_cell(gfx_id, sx, sy, (0, 220, 0), (0, 0, 0));
}

fn discover_service(query: &[u8]) -> Option<u32> {
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

fn grid_size(fb: Option<FramebufferInfo>) -> (usize, usize) {
    if let Some(fb) = fb {
        let cols = (fb.width as usize) / CELL;
        let rows = (fb.height as usize) / CELL;
        return (cols.max(1), rows.max(1));
    }
    (64, 36)
}

fn draw_cell(gfx_id: u32, cx: usize, cy: usize, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
    let x = (cx * CELL) as u16;
    let y = (cy * CELL) as u16;
    let w = CELL as u8;
    let h = CELL as u8;
    let bpr = (CELL + 7) / 8;
    let mut msg = [0u8; 16 + 16];
    msg[..4].copy_from_slice(b"FBDR");
    msg[4..6].copy_from_slice(&x.to_le_bytes());
    msg[6..8].copy_from_slice(&y.to_le_bytes());
    msg[8] = w;
    msg[9] = h;
    msg[10] = fg.0;
    msg[11] = fg.1;
    msg[12] = fg.2;
    msg[13] = bg.0;
    msg[14] = bg.1;
    msg[15] = bg.2;
    for row in 0..CELL {
        msg[16 + row] = 0xFF;
    }
    send_gfx(gfx_id, &msg[..16 + bpr * CELL]);
}

fn send_gfx(gfx_id: u32, data: &[u8]) {
    for _ in 0..64 {
        if syscall::send_message(gfx_id, MessageType::Notification, data).is_ok() {
            return;
        }
        syscall::yield_cpu();
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
    if v == 0 { None } else { Some(v) }
}


// Panic handler provided by libminux.
