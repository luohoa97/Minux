//! x86_64 FPU/SSE/XSAVE bring-up.

use core::arch::asm;
use core::arch::x86_64::__cpuid;

#[inline]
unsafe fn read_cr0() -> u64 {
    let v: u64;
    asm!("mov {}, cr0", out(reg) v, options(nomem, nostack, preserves_flags));
    v
}

#[inline]
unsafe fn write_cr0(v: u64) {
    asm!("mov cr0, {}", in(reg) v, options(nomem, nostack, preserves_flags));
}

#[inline]
unsafe fn read_cr4() -> u64 {
    let v: u64;
    asm!("mov {}, cr4", out(reg) v, options(nomem, nostack, preserves_flags));
    v
}

#[inline]
unsafe fn write_cr4(v: u64) {
    asm!("mov cr4, {}", in(reg) v, options(nomem, nostack, preserves_flags));
}

#[inline]
unsafe fn xgetbv(index: u32) -> u64 {
    let eax: u32;
    let edx: u32;
    asm!("xgetbv", in("ecx") index, out("eax") eax, out("edx") edx, options(nomem, nostack));
    ((edx as u64) << 32) | (eax as u64)
}

#[inline]
unsafe fn xsetbv(index: u32, value: u64) {
    let eax = value as u32;
    let edx = (value >> 32) as u32;
    asm!("xsetbv", in("ecx") index, in("eax") eax, in("edx") edx, options(nomem, nostack));
}

pub fn init() {
    unsafe {
        // Enable x87/SSE support in CR0/CR4.
        // CR0: clear EM, set MP+NE
        let mut cr0 = read_cr0();
        cr0 &= !(1 << 2);
        cr0 |= (1 << 1) | (1 << 5);
        write_cr0(cr0);

        // CR4: OSFXSR (SSE state), OSXMMEXCPT
        let mut cr4 = read_cr4();
        cr4 |= (1 << 9) | (1 << 10);

        // Enable XSAVE if available.
        let leaf1 = __cpuid(1);
        let has_xsave = (leaf1.ecx & (1 << 26)) != 0;
        if has_xsave {
            cr4 |= 1 << 18; // OSXSAVE
        }
        write_cr4(cr4);

        // Init legacy FPU state.
        asm!("fninit", options(nomem, nostack, preserves_flags));

        // Ensure MXCSR has sane defaults.
        let mxcsr: u32 = 0x1f80;
        asm!("ldmxcsr [{}]", in(reg) &mxcsr, options(nostack, preserves_flags));

        if has_xsave {
            // Enable x87 (bit0) + SSE (bit1) in XCR0.
            let mut xcr0 = xgetbv(0);
            xcr0 |= 0b11;
            xsetbv(0, xcr0);
        }
    }
}

pub fn fpu_reset_thread() {
    unsafe {
        asm!("fninit", options(nomem, nostack, preserves_flags));
        let mxcsr: u32 = 0x1f80;
        asm!("ldmxcsr [{}]", in(reg) &mxcsr, options(nostack, preserves_flags));
    }
}

pub unsafe fn fpu_save(area_16b_aligned: *mut u8) {
    asm!("fxsave [{}]", in(reg) area_16b_aligned, options(nostack));
}

pub unsafe fn fpu_restore(area_16b_aligned: *const u8) {
    asm!("fxrstor [{}]", in(reg) area_16b_aligned, options(nostack));
}
