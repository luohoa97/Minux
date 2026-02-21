//! SMP topology discovery and AP startup scaffolding.

use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicU32, Ordering};

use super::lapic;

const MAX_CPUS: usize = 8;
const AP_STACK_SIZE: usize = 16 * 1024;
// Keep AP startup disabled until AP trampoline + per-CPU init path is complete.
// Current staged code can issue INIT/SIPI but does not yet provide a reliable
// low-memory AP bootstrap handoff, which causes machine resets on some VMs.
const ENABLE_AP_STARTUP: bool = false;

#[repr(align(16))]
struct Stack([u8; AP_STACK_SIZE]);

static mut AP_STACKS: [Stack; MAX_CPUS] = [const { Stack([0; AP_STACK_SIZE]) }; MAX_CPUS];
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static AP_ONLINE: AtomicU32 = AtomicU32::new(0);

pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

pub fn ap_online_count() -> u32 {
    AP_ONLINE.load(Ordering::Relaxed)
}

pub fn mark_ap_online(apic_id: u32) {
    let online = AP_ONLINE.fetch_add(1, Ordering::SeqCst) + 1;
    crate::serial_println!("[SMP] AP {} online (ap_online={})", apic_id, online);
}

pub fn init() {
    let leaf1 = unsafe { __cpuid(1) };
    let bsp_apic_id = (leaf1.ebx >> 24) & 0xff;
    let logical = ((leaf1.ebx >> 16) & 0xff).max(1);
    let has_apic = (leaf1.edx & (1 << 9)) != 0;
    let has_x2apic = (leaf1.ecx & (1 << 21)) != 0;
    CPU_COUNT.store(logical.min(MAX_CPUS as u32), Ordering::Relaxed);

    crate::serial_println!(
        "[SMP] BSP apic_id={} logical_cpus={} apic={} x2apic={}",
        bsp_apic_id,
        logical,
        has_apic,
        has_x2apic
    );

    if !has_apic {
        crate::serial_println!("[SMP] No APIC present; SMP disabled");
        return;
    }

    lapic::init(has_x2apic);
    crate::serial_println!("[SMP] Local APIC initialized (id={})", lapic::local_apic_id());
    AP_ONLINE.store(1, Ordering::SeqCst);
    crate::serial_println!(
        "[SMP] CPU {} online (role=BSP, ap_online=1)",
        lapic::local_apic_id()
    );

    if ENABLE_AP_STARTUP {
        bring_up_aps(bsp_apic_id, logical.min(MAX_CPUS as u32));
    } else {
        crate::serial_println!(
            "[SMP] AP startup staged but disabled (ENABLE_AP_STARTUP=false); only BSP scheduler loop is active"
        );
    }
}

fn bring_up_aps(bsp_apic_id: u32, logical: u32) {
    crate::serial_println!("[SMP] Starting AP bring-up via INIT/SIPI");

    // In QEMU default topology, APIC IDs are usually contiguous from 0..N-1.
    for apic_id in 0..logical {
        if apic_id == bsp_apic_id {
            continue;
        }
        prepare_ap_stack(apic_id as usize);
        crate::serial_println!("[SMP] INIT -> apic_id={}", apic_id);
        lapic::send_init_ipi(apic_id);
        delay();
        crate::serial_println!("[SMP] SIPI #1 -> apic_id={} vector=0x08", apic_id);
        lapic::send_startup_ipi(apic_id, 0x08);
        delay();
        crate::serial_println!("[SMP] SIPI #2 -> apic_id={} vector=0x08", apic_id);
        lapic::send_startup_ipi(apic_id, 0x08);
        delay();
    }
}

fn prepare_ap_stack(cpu_index: usize) {
    if cpu_index >= MAX_CPUS {
        return;
    }
    // Stack is pre-allocated; touching it here helps ensure mapping is present.
    unsafe {
        let stack = &mut AP_STACKS[cpu_index].0;
        stack[0] = 0;
        stack[AP_STACK_SIZE - 1] = 0;
    }
}

fn delay() {
    for _ in 0..100_000 {
        core::hint::spin_loop();
    }
}
