//! ELF loader for userspace programs

use crate::microkernel::TaskId;
use crate::mm::AddressSpaceId;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ElfHeader {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;
const DT_NULL: i64 = 0;
const DT_PLTGOT: i64 = 3;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const R_X86_64_RELATIVE: u32 = 8;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
struct DynEntry {
    d_tag: i64,
    d_val: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RelaEntry {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

pub fn load_elf(elf_data: &[u8]) -> Result<(TaskId, u64), ElfError> {
    crate::serial_debugln!("[DBG] elf: load start, size={}", elf_data.len());

    if elf_data.len() < core::mem::size_of::<ElfHeader>() {
        return Err(ElfError::InvalidFormat);
    }
    let header = unsafe { core::ptr::read_unaligned(elf_data.as_ptr() as *const ElfHeader) };

    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidFormat);
    }
    if header.e_ident[4] != 2 || header.e_ident[5] != 1 {
        return Err(ElfError::UnsupportedArch);
    }
    if header.e_machine != 0x3E {
        return Err(ElfError::UnsupportedArch);
    }
    if header.e_phentsize as usize != core::mem::size_of::<ProgramHeader>() {
        return Err(ElfError::InvalidFormat);
    }

    let address_space = crate::mm::create_address_space().map_err(|_| ElfError::OutOfMemory)?;
    let ph_offset = header.e_phoff as usize;
    let ph_size = header.e_phentsize as usize;
    let ph_count = header.e_phnum as usize;
    crate::serial_debugln!("[DBG] elf: phoff=0x{:x} phnum={}", ph_offset, ph_count);

    let mut image_lo = u64::MAX;
    let mut image_hi = 0u64;
    for i in 0..ph_count {
        let ph_addr = ph_offset + i * ph_size;
        if ph_addr + ph_size > elf_data.len() {
            return Err(ElfError::InvalidFormat);
        }
        let ph = unsafe {
            core::ptr::read_unaligned(elf_data.as_ptr().add(ph_addr) as *const ProgramHeader)
        };
        if ph.p_type != PT_LOAD {
            continue;
        }
        let lo = ph.p_vaddr & !0xfff;
        let hi = (ph.p_vaddr + ph.p_memsz + 0xfff) & !0xfff;
        if lo < image_lo {
            image_lo = lo;
        }
        if hi > image_hi {
            image_hi = hi;
        }
    }
    if image_lo == u64::MAX || image_hi <= image_lo {
        return Err(ElfError::InvalidFormat);
    }

    let load_bias = if header.e_type == ET_DYN {
        const USER_DYN_BASE: u64 = 0x0000_2000_0000;
        USER_DYN_BASE.wrapping_sub(image_lo)
    } else {
        0
    };

    for i in 0..ph_count {
        let ph_addr = ph_offset + i * ph_size;
        let ph = unsafe {
            core::ptr::read_unaligned(elf_data.as_ptr().add(ph_addr) as *const ProgramHeader)
        };
        if ph.p_type != PT_LOAD {
            continue;
        }

        crate::serial_debugln!(
            "[DBG] elf: PT_LOAD #{} off=0x{:x} vaddr=0x{:x} filesz=0x{:x} memsz=0x{:x}",
            i,
            ph.p_offset,
            ph.p_vaddr,
            ph.p_filesz,
            ph.p_memsz
        );

        load_segment_mapped(elf_data, &ph, address_space, load_bias)?;
    }
    if header.e_type == ET_DYN {
        apply_relocations(
            elf_data,
            ph_offset,
            ph_size,
            ph_count,
            load_bias,
            address_space,
        )?;
    }

    let task_id =
        crate::microkernel::create_task(address_space).map_err(|_| ElfError::OutOfMemory)?;

    let entry = header.e_entry.wrapping_add(load_bias);
    let stack_size = 0x10000usize;
    // Keep one extra mapped page above logical stack top as bring-up headroom
    // because user tasks currently run at CPL0 and IRQ stubs may touch rsp+offset.
    let stack_pages = (stack_size / 0x1000) + 1;
    let stack_phys = crate::mm::alloc_static_pages(stack_pages).map_err(|_| ElfError::OutOfMemory)?;
    let stack_top = map_user_stack(address_space, task_id, stack_phys, stack_size)?;

    crate::microkernel::setup_task(task_id, entry, stack_top)
        .map_err(|_| ElfError::OutOfMemory)?;

    crate::serial_debugln!(
        "[DBG] elf: task={} entry=0x{:x} type={}",
        task_id,
        entry,
        header.e_type
    );

    Ok((task_id, entry))
}

fn map_user_stack(
    address_space: AddressSpaceId,
    task_id: TaskId,
    stack_phys: u64,
    stack_size: usize,
) -> Result<u64, ElfError> {
    const STACK_FLAGS: u64 = 0x1 | 0x2 | 0x8; // R|W|USER, NX enforced by MM
    const STACK_SLOT_STRIDE: u64 = 0x20_0000; // 2 MiB per task slot
    const STACK_TOP_BASE: u64 = 0x0000_7000_0000_0000;
    let task_slot = (task_id as u64).saturating_add(1);
    let stack_top = STACK_TOP_BASE.saturating_sub(task_slot * STACK_SLOT_STRIDE);
    let stack_base = stack_top.saturating_sub(stack_size as u64);
    let pages = (stack_size / 0x1000) + 1;
    for i in 0..pages {
        let v = stack_base + (i as u64) * 0x1000;
        let p = stack_phys + (i as u64) * 0x1000;
        crate::mm::map_page(address_space, v, p, STACK_FLAGS).map_err(|_| ElfError::OutOfMemory)?;
    }
    crate::serial_debugln!(
        "[DBG] elf: stack mapped task={} v=[0x{:x}..0x{:x}) p=0x{:x}",
        task_id,
        stack_base,
        stack_top,
        stack_phys
    );
    Ok(stack_top)
}

fn apply_relocations(
    elf_data: &[u8],
    ph_offset: usize,
    ph_size: usize,
    ph_count: usize,
    load_bias: u64,
    address_space: AddressSpaceId,
) -> Result<(), ElfError> {
    let mut dyn_file_off = 0usize;
    let mut dyn_filesz = 0usize;
    for i in 0..ph_count {
        let ph_addr = ph_offset + i * ph_size;
        if ph_addr + ph_size > elf_data.len() {
            return Err(ElfError::InvalidFormat);
        }
        let ph = unsafe {
            core::ptr::read_unaligned(elf_data.as_ptr().add(ph_addr) as *const ProgramHeader)
        };
        if ph.p_type == PT_DYNAMIC {
            dyn_file_off = ph.p_offset as usize;
            dyn_filesz = ph.p_filesz as usize;
            break;
        }
    }
    if dyn_filesz == 0 {
        return Ok(());
    }
    if dyn_file_off.checked_add(dyn_filesz).filter(|&n| n <= elf_data.len()).is_none() {
        return Err(ElfError::InvalidFormat);
    }
    let dyn_count = dyn_filesz / core::mem::size_of::<DynEntry>();

    let mut rela_vaddr = 0u64;
    let mut rela_size = 0usize;
    let mut rela_ent = core::mem::size_of::<RelaEntry>();
    let mut pltgot_vaddr = 0u64;
    for i in 0..dyn_count {
        let off = dyn_file_off + i * core::mem::size_of::<DynEntry>();
        let d = unsafe { core::ptr::read_unaligned(elf_data.as_ptr().add(off) as *const DynEntry) };
        match d.d_tag {
            DT_NULL => break,
            DT_PLTGOT => pltgot_vaddr = d.d_val,
            DT_RELA => rela_vaddr = d.d_val,
            DT_RELASZ => rela_size = d.d_val as usize,
            DT_RELAENT => rela_ent = d.d_val as usize,
            _ => {}
        }
    }
    if rela_vaddr == 0 || rela_size == 0 || rela_ent == 0 {
        return Ok(());
    }
    if pltgot_vaddr != 0 {
        crate::serial_debugln!("[DBG] elf: DT_PLTGOT vaddr=0x{:x}", pltgot_vaddr.wrapping_add(load_bias));
    }

    let rela_off = vaddr_to_file_offset(elf_data, ph_offset, ph_size, ph_count, rela_vaddr)?;
    let rela_count = rela_size / rela_ent;
    let mut applied = 0usize;

    for i in 0..rela_count {
        let ent_off = rela_off + i * rela_ent;
        if ent_off.checked_add(core::mem::size_of::<RelaEntry>()).filter(|&n| n <= elf_data.len()).is_none() {
            return Err(ElfError::InvalidFormat);
        }
        let r = unsafe { core::ptr::read_unaligned(elf_data.as_ptr().add(ent_off) as *const RelaEntry) };
        let r_type = (r.r_info & 0xffff_ffff) as u32;
        if r_type != R_X86_64_RELATIVE {
            continue;
        }
        let where_va = r.r_offset.wrapping_add(load_bias);
        let where_pa = crate::mm::translate_address(address_space, where_va).ok_or(ElfError::OutOfMemory)?;
        let where_ptr = where_pa as *mut u64;
        let value = (r.r_addend as i128 + load_bias as i128) as u64;
        unsafe { core::ptr::write(where_ptr, value) };
        applied += 1;
    }
    if applied > 0 {
        crate::serial_debugln!("[DBG] elf: applied {} RELATIVE relocations", applied);
    }
    Ok(())
}

fn vaddr_to_file_offset(
    elf_data: &[u8],
    ph_offset: usize,
    ph_size: usize,
    ph_count: usize,
    vaddr: u64,
) -> Result<usize, ElfError> {
    for i in 0..ph_count {
        let ph_addr = ph_offset + i * ph_size;
        if ph_addr + ph_size > elf_data.len() {
            return Err(ElfError::InvalidFormat);
        }
        let ph = unsafe {
            core::ptr::read_unaligned(elf_data.as_ptr().add(ph_addr) as *const ProgramHeader)
        };
        if ph.p_type != PT_LOAD || ph.p_filesz == 0 {
            continue;
        }
        let lo = ph.p_vaddr;
        let hi = ph.p_vaddr.wrapping_add(ph.p_filesz);
        if vaddr >= lo && vaddr < hi {
            return Ok((ph.p_offset + (vaddr - lo)) as usize);
        }
    }
    Err(ElfError::InvalidFormat)
}

fn load_segment_mapped(
    elf_data: &[u8],
    ph: &ProgramHeader,
    address_space: AddressSpaceId,
    load_bias: u64,
) -> Result<(), ElfError> {
    if ph.p_filesz > ph.p_memsz {
        return Err(ElfError::InvalidFormat);
    }
    if ph.p_memsz == 0 {
        return Ok(());
    }

    let seg_start = ph.p_vaddr.wrapping_add(load_bias) & !0xfff;
    let seg_end = (ph.p_vaddr.wrapping_add(load_bias) + ph.p_memsz + 0xfff) & !0xfff;
    let page_count = ((seg_end - seg_start) / 0x1000) as usize;

    let mut flags = 0u64;
    if ph.p_flags & PF_R != 0 {
        flags |= 0x1;
    }
    if ph.p_flags & PF_W != 0 {
        flags |= 0x2;
    }
    if ph.p_flags & PF_X != 0 {
        flags |= 0x4;
    }
    flags |= 0x8;

    for i in 0..page_count {
        let v = seg_start + (i * 0x1000) as u64;
        let p = if let Some(existing) = crate::mm::translate_address(address_space, v) {
            existing & !0xfff
        } else {
            let fresh = crate::mm::alloc_static_page().map_err(|_| ElfError::OutOfMemory)?;
            crate::mm::map_page(address_space, v, fresh, flags).map_err(|_| ElfError::OutOfMemory)?;
            fresh
        };
        if i == 0 {
            crate::serial_debugln!("[DBG] elf: map first page v=0x{:x} p=0x{:x} flags=0x{:x}", v, p, flags);
        }
    }

    if ph.p_filesz > 0 {
        let file_offset = ph.p_offset as usize;
        let file_size = ph.p_filesz as usize;
        if file_offset.checked_add(file_size).filter(|&n| n <= elf_data.len()).is_none() {
            return Err(ElfError::InvalidFormat);
        }
        for j in 0..file_size {
            let va = ph.p_vaddr.wrapping_add(load_bias) + j as u64;
            let pa = crate::mm::translate_address(address_space, va).ok_or(ElfError::OutOfMemory)?;
            unsafe {
                core::ptr::write(pa as *mut u8, *elf_data.get_unchecked(file_offset + j));
            }
        }
    }

    if ph.p_memsz > ph.p_filesz {
        for j in (ph.p_filesz as usize)..(ph.p_memsz as usize) {
            let va = ph.p_vaddr.wrapping_add(load_bias) + j as u64;
            let pa = crate::mm::translate_address(address_space, va).ok_or(ElfError::OutOfMemory)?;
            unsafe {
                core::ptr::write(pa as *mut u8, 0);
            }
        }
    }
    crate::serial_debugln!("[DBG] elf: segment mapped");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum ElfError {
    InvalidFormat,
    UnsupportedArch,
    OutOfMemory,
}
