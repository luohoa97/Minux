//! Local APIC access and IPI primitives.

use core::arch::asm;

const IA32_APIC_BASE_MSR: u32 = 0x1B;
const IA32_X2APIC_ICR_MSR: u32 = 0x830;

const APIC_BASE_MSR_ENABLE: u64 = 1 << 11;
const APIC_BASE_MSR_X2APIC: u64 = 1 << 10;
const APIC_DEFAULT_PHYS_BASE: u64 = 0xFEE0_0000;

const APIC_REG_SVR: u32 = 0xF0;
const APIC_REG_ICRLO: u32 = 0x300;
const APIC_REG_ICRHI: u32 = 0x310;

fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | lo as u64
}

fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") lo, in("edx") hi, options(nomem, nostack));
    }
}

fn lapic_base_msr() -> u64 {
    rdmsr(IA32_APIC_BASE_MSR)
}

fn lapic_mmio_base() -> *mut u32 {
    let base = lapic_base_msr() & 0xFFFF_F000;
    let phys = if base == 0 { APIC_DEFAULT_PHYS_BASE } else { base };
    phys as *mut u32
}

fn mmio_read(reg: u32) -> u32 {
    let reg_ptr = unsafe { lapic_mmio_base().add((reg / 4) as usize) };
    unsafe { core::ptr::read_volatile(reg_ptr) }
}

fn mmio_write(reg: u32, value: u32) {
    let reg_ptr = unsafe { lapic_mmio_base().add((reg / 4) as usize) };
    unsafe { core::ptr::write_volatile(reg_ptr, value) };
    // Read back to serialize posted write.
    let _ = mmio_read(APIC_REG_SVR);
}

fn x2apic_enabled() -> bool {
    (lapic_base_msr() & APIC_BASE_MSR_X2APIC) != 0
}

pub fn init(enable_x2apic: bool) {
    let mut base = lapic_base_msr();
    base |= APIC_BASE_MSR_ENABLE;
    if enable_x2apic {
        base |= APIC_BASE_MSR_X2APIC;
    } else {
        base &= !APIC_BASE_MSR_X2APIC;
    }
    wrmsr(IA32_APIC_BASE_MSR, base);

    // Spurious interrupt vector register: software enable APIC (bit 8).
    if x2apic_enabled() {
        // x2APIC SVR is MSR 0x80F.
        wrmsr(0x80F, 0x100 | 0xFF);
    } else {
        mmio_write(APIC_REG_SVR, 0x100 | 0xFF);
    }
}

pub fn local_apic_id() -> u32 {
    if x2apic_enabled() {
        rdmsr(0x802) as u32
    } else {
        (mmio_read(0x20) >> 24) & 0xff
    }
}

pub fn send_ipi(dest_apic_id: u32, vector: u8, delivery_mode: u8, level_assert: bool, level_trigger: bool) {
    if x2apic_enabled() {
        let mut icr = (vector as u64) | ((delivery_mode as u64) << 8);
        if level_trigger {
            icr |= 1 << 15;
        }
        if level_assert {
            icr |= 1 << 14;
        }
        icr |= (dest_apic_id as u64) << 32;
        wrmsr(IA32_X2APIC_ICR_MSR, icr);
        return;
    }

    // xAPIC ICR write sequence.
    mmio_write(APIC_REG_ICRHI, dest_apic_id << 24);
    let mut lo = (vector as u32) | ((delivery_mode as u32) << 8);
    if level_trigger {
        lo |= 1 << 15;
    }
    if level_assert {
        lo |= 1 << 14;
    }
    mmio_write(APIC_REG_ICRLO, lo);
}

pub fn send_init_ipi(dest_apic_id: u32) {
    // Delivery mode INIT=0b101, vector ignored.
    send_ipi(dest_apic_id, 0, 0b101, true, true);
}

pub fn send_startup_ipi(dest_apic_id: u32, vector_4k_page: u8) {
    // Delivery mode STARTUP=0b110
    send_ipi(dest_apic_id, vector_4k_page, 0b110, true, false);
}

