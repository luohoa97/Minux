//! Minux microkernel core
//!
//! The microkernel provides only the essential services:
//! - Memory protection and address spaces
//! - Inter-process communication (IPC)
//! - Basic thread/task scheduling
//! - Interrupt handling and hardware abstraction
//!
//! Everything else (drivers, filesystems, network) runs in userspace

mod task;
mod sched;
mod syscall;
mod elf;
use spin::Mutex;

pub use task::*;
pub use sched::*;
pub use syscall::*;
pub use elf::*;

/// Re-export schedule function for interrupt handlers
pub use sched::schedule;
const MAX_BOOT_MODULES: usize = 32;
static BOOT_MODULES: Mutex<([Option<crate::arch::x86_64::BootModule>; MAX_BOOT_MODULES], usize)> =
    Mutex::new(([None; MAX_BOOT_MODULES], 0));
pub const BOOTFS_MAGIC: &[u8; 8] = b"MINUXFS1";

pub fn bootfs_image() -> Option<&'static [u8]> {
    let store = BOOT_MODULES.lock();
    let (arr, n) = &*store;
    let bootfs = arr
        .iter()
        .take(*n)
        .flatten()
        .find(|m| {
            if m.end <= m.start {
                return false;
            }
            let image = unsafe {
                core::slice::from_raw_parts(
                    m.start as *const u8,
                    core::cmp::min(m.end - m.start, BOOTFS_MAGIC.len()),
                )
            };
            image.len() == BOOTFS_MAGIC.len() && image == BOOTFS_MAGIC
        })?;
    if bootfs.end <= bootfs.start {
        return None;
    }
    let image = unsafe { core::slice::from_raw_parts(bootfs.start as *const u8, bootfs.end - bootfs.start) };
    if image.len() < BOOTFS_MAGIC.len() || &image[..BOOTFS_MAGIC.len()] != BOOTFS_MAGIC {
        return None;
    }
    Some(image)
}

/// Initialize the microkernel
pub fn init() {
    // Initialize task management
    task::init();
    crate::serial_debugln!("[DBG] task::init done");
    
    // Initialize scheduler
    sched::init();
    crate::serial_debugln!("[DBG] sched::init done");
}

/// Load boot modules provided by bootloader
/// 
/// In a pure L4 microkernel, the bootloader (multiboot2/UEFI) loads:
/// - kernel.bin (this microkernel)
/// - display/input/storage drivers (user-space)
/// - fs_server (user-space)
/// - pm_server (user-space)
/// - init (user-space)
/// 
/// The kernel parses the bootloader's module list and:
/// 1. Creates address spaces for each module
/// 2. Maps the module memory into those address spaces
/// 3. Creates threads with entry points
/// 4. Marks them as Ready
/// 
/// The kernel does NOT embed or hardcode any drivers!
pub fn load_boot_modules(boot_info: usize) {
    crate::serial_println!("[KERNEL] Parsing bootloader module list...");
    
    // Detect boot protocol
    let protocol = crate::arch::detect_boot_protocol(boot_info);
    crate::serial_println!("[KERNEL] Boot protocol: {:?}", protocol);
    let fb = crate::arch::x86_64::boot::get_boot_framebuffer(boot_info, protocol);
    crate::arch::x86_64::set_boot_framebuffer(fb);
    if let Some(fb) = fb {
        crate::serial_println!(
            "[KERNEL] Framebuffer: addr=0x{:x} {}x{} pitch={} bpp={}",
            fb.phys_addr,
            fb.width,
            fb.height,
            fb.pitch,
            fb.bpp
        );
    } else {
        crate::serial_println!("[KERNEL] Framebuffer: unavailable");
    }
    
    let modules = crate::arch::get_boot_modules(boot_info, protocol);
    {
        let mut store = BOOT_MODULES.lock();
        let (ref mut arr, ref mut n) = *store;
        *n = 0;
        for m in modules.iter().take(MAX_BOOT_MODULES) {
            arr[*n] = Some(*m);
            *n += 1;
        }
    }
    
    if modules.is_empty() {
        crate::serial_println!("[KERNEL] ERROR: No bootloader modules found");
        crate::kernel_fatal("No root task/module loaded by bootloader");
    }
    
    crate::serial_println!("[KERNEL] Found {} boot modules:", modules.len());
    let mut loaded_count = 0usize;
    
    for (idx, module) in modules.iter().enumerate() {
        crate::serial_println!("[KERNEL]   - {} (0x{:x} - 0x{:x})", 
            module.name, module.start, module.end);

        if module.end <= module.start {
            crate::serial_println!("[KERNEL]     Invalid module range");
            continue;
        }
        
        // Deterministic bootstrap: first two modules are elf_loader and init.
        // Avoid depending on bootloader cmdline string integrity here.
        if idx >= 2 {
            crate::serial_println!("[KERNEL]     deferred (loaded on demand by elf_loader)");
            continue;
        }

        // Load module as ELF binary
        let module_data = unsafe {
            core::slice::from_raw_parts(
                module.start as *const u8,
                module.end - module.start
            )
        };
        
        match load_elf(module_data) {
            Ok((task_id, entry)) => {
                crate::serial_println!(
                    "[KERNEL]     bootstrap[{}] loaded as task {}, entry: 0x{:x}",
                    idx,
                    task_id,
                    entry
                );
                loaded_count += 1;
                
                let _ = set_task_state(task_id, TaskState::Ready);
            }
            Err(e) => {
                crate::serial_println!("[KERNEL]     Failed to load: {:?}", e);
            }
        }
    }

    if loaded_count == 0 {
        crate::kernel_fatal("Bootloader modules present, but none were valid ELF tasks");
    }

    crate::serial_println!(
        "[KERNEL] Boot modules loaded successfully ({} task(s))",
        loaded_count
    );
}

/// Exec a deferred boot module by name using cached bootloader module table.
pub fn exec_boot_module(name: &[u8]) -> Result<TaskId, ()> {
    let store = BOOT_MODULES.lock();
    let (arr, n) = &*store;
    for m in arr.iter().take(*n).flatten() {
        if m.name.as_bytes() == name {
            if m.end <= m.start {
                return Err(());
            }
            let module_data = unsafe {
                core::slice::from_raw_parts(m.start as *const u8, m.end - m.start)
            };
            let (task_id, _entry) = load_elf(module_data).map_err(|_| ())?;
            let _ = set_task_state(task_id, TaskState::Ready);
            crate::serial_println!("[KERNEL] exec module '{}' -> task {}", m.name, task_id);
            return Ok(task_id);
        }
    }
    if let Ok(s) = core::str::from_utf8(name) {
        crate::serial_println!("[KERNEL] exec deferred module '{}' (search bootfs)", s);
    }
    if let Some(elf_bytes) = find_bootfs_entry(arr, *n, name) {
        let (task_id, _entry) = load_elf(elf_bytes).map_err(|_| ())?;
        let _ = set_task_state(task_id, TaskState::Ready);
        let name_str = core::str::from_utf8(name).unwrap_or("<nonutf8>");
        crate::serial_println!("[KERNEL] exec bootfs '{}' -> task {}", name_str, task_id);
        return Ok(task_id);
    }
    if let Ok(s) = core::str::from_utf8(name) {
        crate::serial_println!("[KERNEL] exec module '{}' not found", s);
    } else {
        crate::serial_println!("[KERNEL] exec module <nonutf8> not found");
    }
    Err(())
}

fn find_bootfs_entry(
    arr: &[Option<crate::arch::x86_64::BootModule>; MAX_BOOT_MODULES],
    n: usize,
    wanted: &[u8],
) -> Option<&'static [u8]> {
    let bootfs = arr
        .iter()
        .take(n)
        .flatten()
        .find(|m| {
            if m.end <= m.start {
                return false;
            }
            let image = unsafe {
                core::slice::from_raw_parts(m.start as *const u8, core::cmp::min(m.end - m.start, BOOTFS_MAGIC.len()))
            };
            image.len() == BOOTFS_MAGIC.len() && image == BOOTFS_MAGIC
        })?;
    if bootfs.end <= bootfs.start {
        return None;
    }
    let image = unsafe { core::slice::from_raw_parts(bootfs.start as *const u8, bootfs.end - bootfs.start) };
    if image.len() < BOOTFS_MAGIC.len() || &image[..BOOTFS_MAGIC.len()] != BOOTFS_MAGIC {
        return None;
    }
    let mut off = BOOTFS_MAGIC.len();
    while off + 6 <= image.len() {
        let name_len = u16::from_le_bytes([image[off], image[off + 1]]) as usize;
        off += 2;
        let size = u32::from_le_bytes([image[off], image[off + 1], image[off + 2], image[off + 3]]) as usize;
        off += 4;
        if off + name_len > image.len() {
            return None;
        }
        let name = &image[off..off + name_len];
        off += name_len;
        if off + size > image.len() {
            return None;
        }
        let data = &image[off..off + size];
        off += size;
        if name == wanted {
            return Some(data);
        }
    }
    None
}

/// Microkernel main loop
pub fn run() -> ! {
    crate::serial_debugln!("[DBG] entering microkernel::run loop");
    // Kick the first runnable task once so execution leaves pure kernel idle.
    sched::schedule();
    loop {
        // Check for IPC messages
        crate::ipc::process_messages();

        // Bring-up assist: always run scheduler from kernel loop, even when
        // interrupts are enabled. This keeps user-space boot sequencing alive
        // if PIT/LAPIC timer preemption is not yet reliable.
        sched::schedule();

        if crate::arch::interrupts_enabled() {
            // Preemptive mode: scheduling occurs on timer IRQ quanta.
            crate::arch::halt();
        } else {
            // Bring-up fallback: cooperative spin.
            core::hint::spin_loop();
        }
    }
}
