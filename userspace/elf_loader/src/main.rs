#![no_std]
#![no_main]

use libminux::{syscall, MessageType};

const INIT_TASK_ID: u32 = 2;
const FALLBACK_FS_TASK_ID: u32 = 4;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTGOT: i64 = 3;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

#[repr(C)]
#[derive(Clone, Copy)]
struct ElfHeader {
    e_ident: [u8; 16],
    _e_type: u16,
    e_machine: u16,
    _e_version: u32,
    _e_entry: u64,
    e_phoff: u64,
    _e_shoff: u64,
    _e_flags: u32,
    _e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    _e_shentsize: u16,
    _e_shnum: u16,
    _e_shstrndx: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ProgramHeader {
    p_type: u32,
    _p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    _p_paddr: u64,
    p_filesz: u64,
    _p_memsz: u64,
    _p_align: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DynEntry {
    d_tag: i64,
    d_val: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"register:loader:elf_loader");

    loop {
        let mut buf = [0u8; 128];
        match syscall::receive_message(&mut buf) {
            Ok((sender, msg_type)) => {
                let reply_to = if sender == 0 { INIT_TASK_ID } else { sender };
                if !matches!(msg_type, MessageType::Request) {
                    continue;
                }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let data = &buf[..len];

                if let Some(name) = strip_prefix(data, b"exec:") {
                    match exec_path_or_module(name) {
                        Ok(task_id) => {
                            let mut out = [0u8; 16];
                            let n = write_u32_ascii(task_id, &mut out);
                            let _ = syscall::reply_message(reply_to, &out[..n]);
                        }
                        Err(_) => {
                            let _ = syscall::reply_message(reply_to, b"ERR");
                        }
                    }
                } else if let Some(name) = strip_prefix(data, b"got:") {
                    let mut out = [0u8; 128];
                    let n = got_map_for_module(name, &mut out);
                    let _ = syscall::reply_message(reply_to, &out[..n]);
                } else {
                    let _ = syscall::reply_message(reply_to, b"BADREQ");
                }
            }
            Err(_) => syscall::yield_cpu(),
        }
    }
}

fn got_map_for_module(name: &[u8], out: &mut [u8]) -> usize {
    let mut image = [0u8; 32 * 1024];
    let n = match syscall::bootfs_read(name, &mut image) {
        Some(v) if v > 0 && v <= image.len() => v,
        _ => return write_ascii(out, b"ERR:READ"),
    };
    match parse_got_and_needed(&image[..n], out) {
        Some(w) => w,
        None => write_ascii(out, b"ERR:ELF"),
    }
}

fn parse_got_and_needed(elf: &[u8], out: &mut [u8]) -> Option<usize> {
    if elf.len() < core::mem::size_of::<ElfHeader>() {
        return None;
    }
    let eh = unsafe { core::ptr::read_unaligned(elf.as_ptr() as *const ElfHeader) };
    if eh.e_ident[0..4] != ELF_MAGIC || eh.e_ident[4] != 2 || eh.e_ident[5] != 1 || eh.e_machine != 0x3E {
        return None;
    }
    if eh.e_phentsize as usize != core::mem::size_of::<ProgramHeader>() {
        return None;
    }
    let phoff = eh.e_phoff as usize;
    let phent = eh.e_phentsize as usize;
    let phnum = eh.e_phnum as usize;

    let mut dyn_off = 0usize;
    let mut dyn_sz = 0usize;
    for i in 0..phnum {
        let at = phoff + i * phent;
        if at.checked_add(phent).filter(|&k| k <= elf.len()).is_none() {
            return None;
        }
        let ph = unsafe { core::ptr::read_unaligned(elf.as_ptr().add(at) as *const ProgramHeader) };
        if ph.p_type == PT_DYNAMIC {
            dyn_off = ph.p_offset as usize;
            dyn_sz = ph.p_filesz as usize;
            break;
        }
    }
    if dyn_sz == 0 {
        return Some(write_ascii(out, b"NODYN"));
    }
    if dyn_off.checked_add(dyn_sz).filter(|&k| k <= elf.len()).is_none() {
        return None;
    }

    let mut strtab_vaddr = 0u64;
    let mut strsz = 0usize;
    let mut got_vaddr = 0u64;
    let mut needed = [0u64; 8];
    let mut needed_n = 0usize;
    let dcount = dyn_sz / core::mem::size_of::<DynEntry>();
    for i in 0..dcount {
        let at = dyn_off + i * core::mem::size_of::<DynEntry>();
        let d = unsafe { core::ptr::read_unaligned(elf.as_ptr().add(at) as *const DynEntry) };
        match d.d_tag {
            DT_NULL => break,
            DT_NEEDED => {
                if needed_n < needed.len() {
                    needed[needed_n] = d.d_val;
                    needed_n += 1;
                }
            }
            DT_PLTGOT => got_vaddr = d.d_val,
            DT_STRTAB => strtab_vaddr = d.d_val,
            DT_STRSZ => strsz = d.d_val as usize,
            _ => {}
        }
    }

    let mut n = 0usize;
    n += write_ascii(&mut out[n..], b"GOT:");
    if got_vaddr == 0 {
        n += write_ascii(&mut out[n..], b"none");
    } else {
        n += write_hex_u64(got_vaddr, &mut out[n..]);
    }
    n += write_ascii(&mut out[n..], b";NEEDED:");
    if needed_n == 0 {
        n += write_ascii(&mut out[n..], b"none");
        return Some(n);
    }

    let strtab_off = vaddr_to_file_offset(elf, phoff, phent, phnum, strtab_vaddr)?;
    if strtab_off >= elf.len() {
        return None;
    }
    let strtab_end = core::cmp::min(elf.len(), strtab_off.saturating_add(strsz));
    let strtab = &elf[strtab_off..strtab_end];
    for i in 0..needed_n {
        if i > 0 {
            n += write_ascii(&mut out[n..], b",");
        }
        let off = needed[i] as usize;
        n += write_cstr_at(strtab, off, &mut out[n..]);
    }
    Some(n)
}

fn vaddr_to_file_offset(elf: &[u8], phoff: usize, phent: usize, phnum: usize, vaddr: u64) -> Option<usize> {
    for i in 0..phnum {
        let at = phoff + i * phent;
        if at.checked_add(phent).filter(|&k| k <= elf.len()).is_none() {
            return None;
        }
        let ph = unsafe { core::ptr::read_unaligned(elf.as_ptr().add(at) as *const ProgramHeader) };
        if ph.p_type != PT_LOAD || ph.p_filesz == 0 {
            continue;
        }
        let lo = ph.p_vaddr;
        let hi = ph.p_vaddr.wrapping_add(ph.p_filesz);
        if vaddr >= lo && vaddr < hi {
            return Some((ph.p_offset + (vaddr - lo)) as usize);
        }
    }
    None
}

fn exec_path_or_module(target: &[u8]) -> Result<u32, ()> {
    if target.starts_with(b"/") {
        let fs = discover_fs().unwrap_or(FALLBACK_FS_TASK_ID);
        let mut req = [0u8; 96];
        req[..8].copy_from_slice(b"resolve:");
        if target.len() + 8 > req.len() {
            return Err(());
        }
        req[8..8 + target.len()].copy_from_slice(target);
        let _ = syscall::send_message(fs, MessageType::Request, &req[..8 + target.len()]);
        let mut reply = [0u8; 64];
        if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
            let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
            let name = &reply[..len];
            if name == b"NOTFOUND" || name.is_empty() {
                return Err(());
            }
            return syscall::exec_module(name);
        }
        Err(())
    } else {
        syscall::exec_module(target)
    }
}

fn discover_fs() -> Option<u32> {
    let _ = syscall::send_message(INIT_TASK_ID, MessageType::Request, b"lookup:vfs");
    let mut reply = [0u8; 16];
    if let Ok((_sender, MessageType::Reply)) = syscall::receive_message(&mut reply) {
        let len = reply.iter().position(|&b| b == 0).unwrap_or(reply.len());
        return parse_u32_ascii(&reply[..len]);
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

fn strip_prefix<'a>(data: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    if data.len() < p.len() || &data[..p.len()] != p { None } else { Some(&data[p.len()..]) }
}

fn write_u32_ascii(id: u32, out: &mut [u8]) -> usize {
    if out.is_empty() { return 0; }
    if id == 0 { out[0] = b'0'; return 1; }
    let mut tmp = [0u8; 10];
    let mut n = id;
    let mut k = 0usize;
    while n > 0 && k < tmp.len() {
        tmp[k] = b'0' + (n % 10) as u8;
        n /= 10;
        k += 1;
    }
    let len = core::cmp::min(k, out.len());
    for j in 0..len { out[j] = tmp[k - 1 - j]; }
    len
}

fn write_ascii(out: &mut [u8], s: &[u8]) -> usize {
    let n = core::cmp::min(out.len(), s.len());
    out[..n].copy_from_slice(&s[..n]);
    n
}

fn write_hex_u64(v: u64, out: &mut [u8]) -> usize {
    if out.len() < 3 {
        return 0;
    }
    let mut n = 0usize;
    out[n] = b'0';
    n += 1;
    out[n] = b'x';
    n += 1;
    let mut started = false;
    for shift in (0..16).rev() {
        let d = ((v >> (shift * 4)) & 0xf) as u8;
        if !started && d == 0 && shift != 0 {
            continue;
        }
        started = true;
        if n >= out.len() {
            break;
        }
        out[n] = match d {
            0..=9 => b'0' + d,
            _ => b'a' + (d - 10),
        };
        n += 1;
    }
    n
}

fn write_cstr_at(src: &[u8], at: usize, out: &mut [u8]) -> usize {
    if at >= src.len() || out.is_empty() {
        return 0;
    }
    let mut i = at;
    let mut n = 0usize;
    while i < src.len() && n < out.len() {
        let b = src[i];
        if b == 0 {
            break;
        }
        out[n] = b;
        n += 1;
        i += 1;
    }
    n
}
