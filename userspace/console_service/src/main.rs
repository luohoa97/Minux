//! Console (TTY) service.
//! Owns terminal semantics (cursor/scroll/UTF-8/ANSI/PSF2) and emits draw commands to gfx.

#![no_std]
#![no_main]

use libminux::{syscall, FramebufferInfo, MessageType, TaskId};

const INIT_TASK_ID: TaskId = 2;
const FALLBACK_GFX_ID: TaskId = 3;
const NUM_TTYS: usize = 6;
const PSF2_MAGIC: u32 = 0x864A_B572;
const MAX_ROWS: usize = 128;
const MAX_COLS: usize = 160;
const MAX_CSI_PARAMS: usize = 4;
const TTY_LINE_MAX: usize = 60;
const TTY_DEBUG: bool = false;

#[repr(C)]
#[derive(Clone, Copy)]
struct Psf2Header {
    magic: u32,
    version: u32,
    headersize: u32,
    flags: u32,
    length: u32,
    charsize: u32,
    height: u32,
    width: u32,
}

#[derive(Clone, Copy)]
struct LoadedFont {
    name: &'static [u8],
    data: &'static [u8],
    header: Psf2Header,
    glyph_map: [u16; 256],
}

#[derive(Clone, Copy)]
struct Terminal {
    cols: usize,
    rows: usize,
    cx: usize,
    cy: usize,
    line_len: [u16; MAX_ROWS],
    screen: [[u8; MAX_COLS]; MAX_ROWS],
    esc: u8, // 0=normal,1=ESC,2=CSI
    csi_vals: [u16; MAX_CSI_PARAMS],
    csi_count: usize,
    utf_code: u32,
    utf_need: u8,
}

impl Terminal {
    const fn new() -> Self {
        Self {
            cols: 80,
            rows: 25,
            cx: 0,
            cy: 0,
            line_len: [0; MAX_ROWS],
            screen: [[b' '; MAX_COLS]; MAX_ROWS],
            esc: 0,
            csi_vals: [0; MAX_CSI_PARAMS],
            csi_count: 0,
            utf_code: 0,
            utf_need: 0,
        }
    }
}

static FONT_TER_U16N: &[u8] = include_bytes!("../../assets/fonts/ter-u16n.psf");
static FONT_TER_U16B: &[u8] = include_bytes!("../../assets/fonts/ter-u16b.psf");

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:tty:console_service");

    let gfx_id = wait_for_service(b"lookup:gfx").unwrap_or(FALLBACK_GFX_ID);
    let mut input_id = wait_for_service(b"lookup:input").unwrap_or(INIT_TASK_ID);
    let fb = syscall::get_framebuffer_info();
    let mut terminal = Terminal::new();
    let mut active_font = load_builtin_font(b"ter-u16n");
    if let Some(font) = active_font {
        set_terminal_geometry(&mut terminal, fb, font);
        draw_clear(gfx_id);
        draw_text(gfx_id, &mut terminal, font, b"[CONSOLE] tty online\nWelcome to Minux\n");
    }

    let mut current_tty: usize = 0;
    let mut foreground: [Option<TaskId>; NUM_TTYS] = [None; NUM_TTYS];
    let mut raw_mode: [bool; NUM_TTYS] = [true; NUM_TTYS];
    let mut echo_mode: [bool; NUM_TTYS] = [true; NUM_TTYS];
    let mut line_len: [usize; NUM_TTYS] = [0; NUM_TTYS];
    let mut line_buf = [[0u8; TTY_LINE_MAX]; NUM_TTYS];
    let mut input_count: u32 = 0;

    let mut probe_tick: u32 = 0;
    loop {
        if (probe_tick & 0x3ff) == 0 {
            if let Some(id) = discover_service(b"lookup:input") {
                input_id = id;
            }
        }
        probe_tick = probe_tick.wrapping_add(1);
        let mut buf = [0u8; 192];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if matches!(msg_type, MessageType::Request) {
                    if data == b"font:list" {
                        let _ = syscall::reply_message(sender, b"ter-u16n,ter-u16b");
                        continue;
                    }
                    if data == b"font:info" {
                        if let Some(font) = active_font {
                            let mut msg = [0u8; 80];
                            let mut n = 0usize;
                            n += write_ascii(&mut msg[n..], b"FONT:");
                            n += write_ascii(&mut msg[n..], font.name);
                            n += write_ascii(&mut msg[n..], b" ");
                            n += write_u32_ascii(font.header.width, &mut msg[n..]);
                            n += write_ascii(&mut msg[n..], b"x");
                            n += write_u32_ascii(font.header.height, &mut msg[n..]);
                            let _ = syscall::reply_message(sender, &msg[..n]);
                        } else {
                            let _ = syscall::reply_message(sender, b"FONT:NONE");
                        }
                        continue;
                    }
                    if data.len() > 10 && &data[..10] == b"font:load:" {
                        if let Some(font) = load_builtin_font(&data[10..]) {
                            active_font = Some(font);
                            set_terminal_geometry(&mut terminal, fb, font);
                            draw_clear(gfx_id);
                            let _ = syscall::reply_message(sender, b"FONT:OK");
                        } else {
                            let _ = syscall::reply_message(sender, b"FONT:NF");
                        }
                        continue;
                    }
                }

                if data == b"tty:acquire" {
                    if foreground[current_tty].is_none() || foreground[current_tty] == Some(sender) {
                        foreground[current_tty] = Some(sender);
                        let _ = syscall::reply_message(sender, b"TTY:FG");
                    } else {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                    }
                    continue;
                }

                if data == b"tty:release" {
                    if foreground[current_tty] == Some(sender) {
                        foreground[current_tty] = None;
                    }
                    let _ = syscall::reply_message(sender, b"TTY:REL");
                    continue;
                }
                if data == b"tty:winsize" {
                    if foreground[current_tty].is_some() && foreground[current_tty] != Some(sender) {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        continue;
                    }
                    let mut msg = [0u8; 24];
                    let mut n = 0usize;
                    n += write_ascii(&mut msg[n..], b"TTY:WS:");
                    n += write_u32_ascii(terminal.cols as u32, &mut msg[n..]);
                    n += write_ascii(&mut msg[n..], b"x");
                    n += write_u32_ascii(terminal.rows as u32, &mut msg[n..]);
                    let _ = syscall::reply_message(sender, &msg[..n]);
                    continue;
                }
                if data == b"tty:mode:raw" {
                    if foreground[current_tty].is_some() && foreground[current_tty] != Some(sender) {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        continue;
                    }
                    raw_mode[current_tty] = true;
                    line_len[current_tty] = 0;
                    let _ = syscall::reply_message(sender, b"TTY:OK");
                    continue;
                }
                if data == b"tty:mode:cooked" {
                    if foreground[current_tty].is_some() && foreground[current_tty] != Some(sender) {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        continue;
                    }
                    raw_mode[current_tty] = false;
                    line_len[current_tty] = 0;
                    let _ = syscall::reply_message(sender, b"TTY:OK");
                    continue;
                }
                if data == b"tty:echo:on" {
                    if foreground[current_tty].is_some() && foreground[current_tty] != Some(sender) {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        continue;
                    }
                    echo_mode[current_tty] = true;
                    let _ = syscall::reply_message(sender, b"TTY:OK");
                    continue;
                }
                if data == b"tty:echo:off" {
                    if foreground[current_tty].is_some() && foreground[current_tty] != Some(sender) {
                        let _ = syscall::reply_message(sender, b"TTY:BUSY");
                        continue;
                    }
                    echo_mode[current_tty] = false;
                    let _ = syscall::reply_message(sender, b"TTY:OK");
                    continue;
                }

                if sender == input_id {
                    if data.len() == 1 {
                        input_count = input_count.wrapping_add(1);
                        if TTY_DEBUG && (input_count & 0x1f) == 0 {
                            log_tty_input(gfx_id, data[0], input_count, foreground[current_tty], raw_mode[current_tty]);
                        }
                        if let Some(owner) = foreground[current_tty] {
                            if raw_mode[current_tty] {
                                send_task(owner, MessageType::Notification as u32, data);
                            } else {
                                handle_cooked_input(
                                    owner,
                                    gfx_id,
                                    &mut terminal,
                                    active_font,
                                    data[0],
                                    echo_mode[current_tty],
                                    &mut line_buf[current_tty],
                                    &mut line_len[current_tty],
                                );
                            }
                        }
                        continue;
                    }
                    if let Some(next_tty) = parse_tty_switch(data) {
                        if next_tty < NUM_TTYS {
                            current_tty = next_tty;
                            draw_clear(gfx_id);
                            if let Some(font) = active_font {
                                terminal = Terminal::new();
                                set_terminal_geometry(&mut terminal, fb, font);
                            }
                            line_len[current_tty] = 0;
                            let _ = syscall::reply_message(sender, b"TTY:SWITCHED");
                        } else {
                            let _ = syscall::reply_message(sender, b"TTY:BAD");
                        }
                        continue;
                    }
                }

                if let Some(owner) = foreground[current_tty] {
                    if owner != sender {
                        // Allow background notifications (write-only) to display.
                        if matches!(msg_type, MessageType::Request) {
                            let _ = syscall::reply_message(sender, b"TTY:BUSY");
                            continue;
                        }
                    }
                }

                match msg_type {
                    MessageType::Notification | MessageType::Request => {
                        if data == b"clear" {
                            draw_clear(gfx_id);
                            if let Some(font) = active_font {
                                terminal = Terminal::new();
                                set_terminal_geometry(&mut terminal, fb, font);
                            }
                        } else if let Some(font) = active_font {
                            draw_text(gfx_id, &mut terminal, font, data);
                        }
                        if matches!(msg_type, MessageType::Request) {
                            let _ = syscall::reply_message(sender, b"TTY:OK");
                        }
                    }
                    _ => {}
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn log_tty_input(
    gfx_id: TaskId,
    ch: u8,
    count: u32,
    owner: Option<TaskId>,
    raw: bool,
) {
    let mut msg = [0u8; 64];
    let mut n = 0usize;
    n += write_ascii(&mut msg[n..], b"\n[TTYDBG] ch=");
    n += write_hex_u8(ch, &mut msg[n..]);
    n += write_ascii(&mut msg[n..], b" n=");
    n += write_u32_ascii(count, &mut msg[n..]);
    n += write_ascii(&mut msg[n..], b" own=");
    n += write_u32_ascii(owner.unwrap_or(0), &mut msg[n..]);
    n += write_ascii(&mut msg[n..], if raw { b" raw" } else { b" cooked" });
    send_gfx(gfx_id, &msg[..n]);
}

fn write_hex_u8(v: u8, out: &mut [u8]) -> usize {
    if out.len() < 2 {
        return 0;
    }
    let hex = b"0123456789ABCDEF";
    out[0] = hex[(v >> 4) as usize];
    out[1] = hex[(v & 0x0F) as usize];
    2
}

fn set_terminal_geometry(t: &mut Terminal, fb: Option<FramebufferInfo>, font: LoadedFont) {
    if let Some(fb) = fb {
        t.cols = core::cmp::min(
            core::cmp::max((fb.width as usize) / (font.header.width as usize), 1),
            MAX_COLS,
        );
        t.rows = core::cmp::min(core::cmp::max((fb.height as usize) / (font.header.height as usize), 1), MAX_ROWS);
    } else {
        t.cols = core::cmp::min(80, MAX_COLS);
        t.rows = 25;
    }
}

fn draw_clear(gfx_id: TaskId) {
    send_gfx(gfx_id, b"FBCL");
}

fn clear_screen_buffer(t: &mut Terminal) {
    for y in 0..t.rows {
        for x in 0..t.cols {
            t.screen[y][x] = b' ';
        }
        t.line_len[y] = 0;
    }
}

fn redraw_screen(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont) {
    draw_clear(gfx_id);
    for y in 0..t.rows {
        for x in 0..t.cols {
            let ch = t.screen[y][x];
            if ch != b' ' {
                draw_char(gfx_id, t, font, x, y, ch);
            }
        }
    }
}

fn scroll_up(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont, lines: usize) {
    let l = core::cmp::min(lines, t.rows);
    if l == 0 {
        return;
    }
    for y in 0..(t.rows - l) {
        let (top, bottom) = t.screen.split_at_mut(y + l);
        top[y][..t.cols].copy_from_slice(&bottom[0][..t.cols]);
        t.line_len[y] = t.line_len[y + l];
    }
    for y in (t.rows - l)..t.rows {
        for x in 0..t.cols {
            t.screen[y][x] = b' ';
        }
        t.line_len[y] = 0;
    }
    redraw_screen(gfx_id, t, font);
}

fn draw_text(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont, data: &[u8]) {
    for &b in data {
        if t.esc == 0 {
            if b == 0x1B {
                t.esc = 1;
                continue;
            }
            if t.utf_need == 0 {
                if b < 0x80 {
                    put_codepoint(gfx_id, t, font, b as u32);
                } else if (b & 0xE0) == 0xC0 {
                    t.utf_code = (b & 0x1F) as u32;
                    t.utf_need = 1;
                } else if (b & 0xF0) == 0xE0 {
                    t.utf_code = (b & 0x0F) as u32;
                    t.utf_need = 2;
                } else if (b & 0xF8) == 0xF0 {
                    t.utf_code = (b & 0x07) as u32;
                    t.utf_need = 3;
                } else {
                    put_codepoint(gfx_id, t, font, b'?' as u32);
                }
            } else if (b & 0xC0) == 0x80 {
                t.utf_code = (t.utf_code << 6) | ((b & 0x3F) as u32);
                t.utf_need -= 1;
                if t.utf_need == 0 {
                    put_codepoint(gfx_id, t, font, t.utf_code);
                }
            } else {
                t.utf_need = 0;
                put_codepoint(gfx_id, t, font, b'?' as u32);
            }
            continue;
        }

        if t.esc == 1 {
            if b == b'[' {
                t.esc = 2;
                t.csi_vals = [0; MAX_CSI_PARAMS];
                t.csi_count = 0;
            } else {
                t.esc = 0;
            }
            continue;
        }

        if t.esc == 2 {
            if b.is_ascii_digit() {
                let idx = core::cmp::min(t.csi_count, MAX_CSI_PARAMS - 1);
                t.csi_vals[idx] = t.csi_vals[idx].saturating_mul(10).saturating_add((b - b'0') as u16);
                continue;
            }
            if b == b';' {
                if t.csi_count + 1 < MAX_CSI_PARAMS {
                    t.csi_count += 1;
                }
                continue;
            }
            apply_csi(gfx_id, t, font, b);
            t.esc = 0;
        }
    }
}

fn apply_csi(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont, op: u8) {
    let p0 = if t.csi_vals[0] == 0 { 1 } else { t.csi_vals[0] as usize };
    let p1 = if t.csi_vals[1] == 0 { 1 } else { t.csi_vals[1] as usize };
    match op {
        b'A' => t.cy = t.cy.saturating_sub(p0),
        b'B' => t.cy = core::cmp::min(t.cy + p0, t.rows - 1),
        b'C' => t.cx = core::cmp::min(t.cx + p0, t.cols - 1),
        b'D' => t.cx = t.cx.saturating_sub(p0),
        b'H' | b'f' => {
            t.cy = core::cmp::min(p0.saturating_sub(1), t.rows - 1);
            t.cx = core::cmp::min(p1.saturating_sub(1), t.cols - 1);
        }
        b'J' => {
            draw_clear(gfx_id);
            t.cx = 0;
            t.cy = 0;
            clear_screen_buffer(t);
        }
        b'K' => {
            for x in t.cx..t.cols {
                draw_char(gfx_id, t, font, x, t.cy, b' ');
            }
            if t.cy < MAX_ROWS {
                t.line_len[t.cy] = t.cx as u16;
            }
        }
        _ => {}
    }
}

fn put_codepoint(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont, cp: u32) {
    match cp {
        0x0A => {
            if t.cy < MAX_ROWS && t.cx > t.line_len[t.cy] as usize {
                t.line_len[t.cy] = t.cx as u16;
            }
            t.cx = 0;
            t.cy += 1;
            if t.cy >= t.rows {
                scroll_up(gfx_id, t, font, 1);
                t.cy = t.rows.saturating_sub(1);
            }
        }
        0x0D => t.cx = 0, // carriage return overwrite
        0x09 => {
            let next = ((t.cx / 8) + 1) * 8;
            while t.cx < next {
                put_codepoint(gfx_id, t, font, b' ' as u32);
            }
        }
        0x08 => {
            if t.cx > 0 {
                t.cx -= 1;
            } else if t.cy > 0 {
                t.cy -= 1;
                t.cx = t.line_len[t.cy] as usize;
                if t.cx > 0 {
                    t.cx -= 1;
                }
            } else {
                return;
            }
            draw_char(gfx_id, t, font, t.cx, t.cy, b' ');
            if t.cy < MAX_ROWS && t.cx < t.line_len[t.cy] as usize {
                t.line_len[t.cy] = t.cx as u16;
            }
        }
        _ => {
            let ch = if cp <= 0xFF { cp as u8 } else { b'?' };
            draw_char(gfx_id, t, font, t.cx, t.cy, ch);
            t.cx += 1;
            if t.cy < MAX_ROWS && t.cx > t.line_len[t.cy] as usize {
                t.line_len[t.cy] = t.cx as u16;
            }
            if t.cx >= t.cols {
                t.cx = 0;
                t.cy += 1;
                if t.cy >= t.rows {
                    scroll_up(gfx_id, t, font, 1);
                    t.cy = t.rows.saturating_sub(1);
                }
            }
        }
    }
}

fn draw_char(gfx_id: TaskId, t: &mut Terminal, font: LoadedFont, col: usize, row: usize, ch: u8) {
    if row < t.rows && col < t.cols {
        t.screen[row][col] = ch;
    }
    let gw = font.header.width as usize;
    let gh = font.header.height as usize;
    let bpr = (gw + 7) / 8;
    if gw == 0 || gh == 0 || bpr == 0 {
        return;
    }
    let glyph_idx = font.glyph_map[ch as usize] as usize;
    let off = font.header.headersize as usize + glyph_idx * font.header.charsize as usize;
    if off + font.header.charsize as usize > font.data.len() {
        return;
    }
    let g = &font.data[off..off + font.header.charsize as usize];
    let x = (col * gw) as u16;
    let y = (row * gh) as u16;
    // Kernel IPC payload is capped at 64 bytes; FBDR header is 16 bytes.
    // Send glyph bitmap in row chunks so large glyphs never overflow.
    const FBDR_HDR: usize = 16;
    const IPC_MAX: usize = 64;
    let max_bitmap = IPC_MAX - FBDR_HDR;
    let rows_per_msg = core::cmp::max(1, max_bitmap / bpr);
    let rows_available = core::cmp::min(gh, g.len() / bpr);
    let mut row_off = 0usize;
    while row_off < rows_available {
        let chunk_rows = core::cmp::min(rows_per_msg, rows_available - row_off);
        let chunk_bytes = chunk_rows * bpr;
        let src = row_off * bpr;
        let mut msg = [0u8; IPC_MAX];
        msg[..4].copy_from_slice(b"FBDR");
        msg[4..6].copy_from_slice(&x.to_le_bytes());
        msg[6..8].copy_from_slice(&(y + row_off as u16).to_le_bytes());
        msg[8] = gw as u8;
        msg[9] = chunk_rows as u8;
        msg[10] = 220;
        msg[11] = 220;
        msg[12] = 220;
        msg[13] = 0;
        msg[14] = 0;
        msg[15] = 0;
        msg[FBDR_HDR..FBDR_HDR + chunk_bytes].copy_from_slice(&g[src..src + chunk_bytes]);
        send_gfx(gfx_id, &msg[..FBDR_HDR + chunk_bytes]);
        row_off += chunk_rows;
    }
}

fn send_gfx(gfx_id: TaskId, data: &[u8]) {
    // Rendering can burst many small packets; tolerate transient queue-full.
    for _ in 0..64 {
        if syscall::send_message(gfx_id, MessageType::Notification, data).is_ok() {
            return;
        }
        syscall::yield_cpu();
    }
}

fn send_task(target: TaskId, ty: u32, data: &[u8]) {
    for _ in 0..64 {
        let msg_type = match ty {
            0 => MessageType::Request,
            1 => MessageType::Reply,
            2 => MessageType::Notification,
            3 => MessageType::Interrupt,
            _ => MessageType::Notification,
        };
        if syscall::send_message(target, msg_type, data).is_ok() {
            return;
        }
        syscall::yield_cpu();
    }
}

fn handle_cooked_input(
    owner: TaskId,
    gfx_id: TaskId,
    term: &mut Terminal,
    font: Option<LoadedFont>,
    ch: u8,
    echo: bool,
    line_buf: &mut [u8; TTY_LINE_MAX],
    line_len: &mut usize,
) {
    match ch {
        0x03 => {
            // Ctrl+C: deliver interrupt byte immediately and reset current line.
            *line_len = 0;
            send_task(owner, MessageType::Notification as u32, &[0x03]);
            if echo {
                if let Some(f) = font {
                    draw_text(gfx_id, term, f, b"^C\n");
                }
            }
        }
        0x04 => {
            // Ctrl+D: canonical EOF marker.
            send_task(owner, MessageType::Notification as u32, &[0x04]);
        }
        0x08 => {
            if *line_len > 0 {
                *line_len -= 1;
                if echo {
                    if let Some(f) = font {
                        draw_text(gfx_id, term, f, b"\x08 \x08");
                    }
                }
            }
        }
        b'\r' | b'\n' => {
            if *line_len < TTY_LINE_MAX {
                line_buf[*line_len] = b'\n';
                *line_len += 1;
            }
            send_task(owner, MessageType::Notification as u32, &line_buf[..*line_len]);
            *line_len = 0;
            if echo {
                if let Some(f) = font {
                    draw_text(gfx_id, term, f, b"\n");
                }
            }
        }
        b if b == b'\t' || (0x20..=0x7e).contains(&b) => {
            if *line_len < TTY_LINE_MAX {
                line_buf[*line_len] = b;
                *line_len += 1;
                if echo {
                    if let Some(f) = font {
                        draw_text(gfx_id, term, f, &[b]);
                    }
                }
            }
        }
        _ => {}
    }
}

fn load_builtin_font(name: &[u8]) -> Option<LoadedFont> {
    let (font_name, data) = if name == b"ter-u16n" {
        (b"ter-u16n".as_slice(), FONT_TER_U16N)
    } else if name == b"ter-u16b" {
        (b"ter-u16b".as_slice(), FONT_TER_U16B)
    } else {
        return None;
    };
    let header = parse_psf2_header(data)?;
    let glyph_map = build_glyph_map(data, header);
    Some(LoadedFont { name: font_name, data, header, glyph_map })
}

fn parse_psf2_header(data: &[u8]) -> Option<Psf2Header> {
    if data.len() < 32 {
        return None;
    }
    let h = Psf2Header {
        magic: le_u32(&data[0..4]),
        version: le_u32(&data[4..8]),
        headersize: le_u32(&data[8..12]),
        flags: le_u32(&data[12..16]),
        length: le_u32(&data[16..20]),
        charsize: le_u32(&data[20..24]),
        height: le_u32(&data[24..28]),
        width: le_u32(&data[28..32]),
    };
    if h.magic != PSF2_MAGIC || h.length == 0 || h.charsize == 0 || h.height == 0 || h.width == 0 {
        return None;
    }
    let glyph_bytes = (h.length as usize).checked_mul(h.charsize as usize)?;
    if (h.headersize as usize).checked_add(glyph_bytes)? > data.len() {
        return None;
    }
    Some(h)
}

fn build_glyph_map(data: &[u8], h: Psf2Header) -> [u16; 256] {
    let mut map = [0u16; 256];
    for i in 0..256 {
        map[i] = i as u16;
    }
    if (h.flags & 0x1) == 0 {
        return map;
    }
    let mut p = h.headersize as usize + (h.length as usize) * (h.charsize as usize);
    let mut glyph = 0usize;
    while glyph < h.length as usize && p < data.len() {
        let mut seq_start = true;
        while p < data.len() {
            let b = data[p];
            if b == 0xFF {
                p += 1;
                break;
            }
            if b == 0xFE {
                seq_start = true;
                p += 1;
                continue;
            }
            if let Some((cp, n)) = decode_utf8(&data[p..]) {
                if seq_start && cp <= 0xFF {
                    map[cp as usize] = glyph as u16;
                }
                seq_start = false;
                p += n;
            } else {
                p += 1;
            }
        }
        glyph += 1;
    }
    map
}

fn decode_utf8(data: &[u8]) -> Option<(u32, usize)> {
    let b0 = *data.first()?;
    if b0 < 0x80 {
        return Some((b0 as u32, 1));
    }
    if (b0 & 0xE0) == 0xC0 {
        let b1 = *data.get(1)?;
        if (b1 & 0xC0) != 0x80 {
            return None;
        }
        return Some(((((b0 & 0x1F) as u32) << 6) | ((b1 & 0x3F) as u32), 2));
    }
    if (b0 & 0xF0) == 0xE0 {
        let b1 = *data.get(1)?;
        let b2 = *data.get(2)?;
        if (b1 & 0xC0) != 0x80 || (b2 & 0xC0) != 0x80 {
            return None;
        }
        return Some(((((b0 & 0x0F) as u32) << 12) | (((b1 & 0x3F) as u32) << 6) | ((b2 & 0x3F) as u32), 3));
    }
    None
}

fn le_u32(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

fn discover_service(query: &[u8]) -> Option<TaskId> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, query);
    // Init reply may lag behind the request; poll briefly.
    for _ in 0..256u32 {
        let mut reply = [0u8; 16];
        if let Ok((sender, msg_type)) = syscall::receive_message(&mut reply) {
            if sender != INIT_TASK_ID || !matches!(msg_type, MessageType::Reply) {
                continue;
            }
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            let id = parse_u32_ascii(&reply[..len])?;
            if id == INIT_TASK_ID {
                return None;
            }
            return Some(id);
        }
        syscall::yield_cpu();
    }
    None
}

fn wait_for_service(query: &[u8]) -> Option<TaskId> {
    loop {
        if let Some(id) = discover_service(query) {
            return Some(id);
        }
        syscall::yield_cpu();
    }
}

fn parse_tty_switch(data: &[u8]) -> Option<usize> {
    const P: &[u8] = b"tty:switch:";
    if data.len() < P.len() || &data[..P.len()] != P {
        return None;
    }
    let n = parse_u32_ascii(&data[P.len()..])?;
    if n == 0 {
        return None;
    }
    Some((n - 1) as usize)
}

fn parse_u32_ascii(data: &[u8]) -> Option<u32> {
    if data.is_empty() {
        return None;
    }
    let mut v = 0u32;
    for &b in data {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?;
        v = v.checked_add((b - b'0') as u32)?;
    }
    if v == 0 { None } else { Some(v) }
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
