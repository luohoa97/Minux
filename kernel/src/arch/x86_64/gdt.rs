//! Global Descriptor Table and Task State Segment setup.

#[repr(C, packed)]
struct Gdtr {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct Tss64 {
    _reserved1: u32,
    rsp: [u64; 3],
    _reserved2: u64,
    ist: [u64; 7],
    _reserved3: u64,
    _reserved4: u16,
    iomap_base: u16,
}

#[repr(align(16))]
struct AlignedStack<const N: usize>([u8; N]);

const KERNEL_CODE_SELECTOR: u16 = 0x08;
const KERNEL_DATA_SELECTOR: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x18;
const DOUBLE_FAULT_IST_INDEX: usize = 0;
const DOUBLE_FAULT_STACK_SIZE: usize = 4096 * 8;

static mut GDT: [u64; 5] = [0; 5];
static mut TSS: Tss64 = Tss64 {
    _reserved1: 0,
    rsp: [0; 3],
    _reserved2: 0,
    ist: [0; 7],
    _reserved3: 0,
    _reserved4: 0,
    iomap_base: core::mem::size_of::<Tss64>() as u16,
};
static mut DOUBLE_FAULT_STACK: AlignedStack<DOUBLE_FAULT_STACK_SIZE> =
    AlignedStack([0; DOUBLE_FAULT_STACK_SIZE]);

const GDT_KERNEL_CODE: u64 = 0x00af9a000000ffff;
const GDT_KERNEL_DATA: u64 = 0x00af92000000ffff;

fn tss_descriptor(base: u64, limit: u32) -> (u64, u64) {
    let mut low = 0u64;
    low |= (limit as u64) & 0xFFFF;
    low |= (base & 0x00FF_FFFF) << 16;
    low |= 0x89u64 << 40;
    low |= ((limit as u64 >> 16) & 0xF) << 48;
    low |= ((base >> 24) & 0xFF) << 56;
    let high = base >> 32;
    (low, high)
}

fn load_segments_and_tss() {
    unsafe {
        core::arch::asm!(
            "mov ax, {data_sel}",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "push {code_sel}",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, {tss_sel}",
            "ltr ax",
            data_sel = const KERNEL_DATA_SELECTOR,
            code_sel = const KERNEL_CODE_SELECTOR as u64,
            tss_sel = const TSS_SELECTOR,
            out("rax") _,
            options(preserves_flags)
        );
    }
}

pub fn dump_gdtr() {
    let mut gdtr = Gdtr { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!("sgdt [{0}]", in(reg) &mut gdtr, options(nostack, preserves_flags));
    }
    let base = gdtr.base;
    let limit = gdtr.limit;
    crate::serial_debugln!("[DBG] GDTR base={:#x} limit={:#x}", base, limit);
}

pub fn init() {
    dump_gdtr();

    unsafe {
        let df_stack_top =
            core::ptr::addr_of!(DOUBLE_FAULT_STACK.0) as u64 + DOUBLE_FAULT_STACK_SIZE as u64;
        TSS.ist[DOUBLE_FAULT_IST_INDEX] = df_stack_top;

        GDT[0] = 0;
        GDT[1] = GDT_KERNEL_CODE;
        GDT[2] = GDT_KERNEL_DATA;

        let tss_base = core::ptr::addr_of!(TSS) as u64;
        let tss_limit = (core::mem::size_of::<Tss64>() - 1) as u32;
        let (tss_low, tss_high) = tss_descriptor(tss_base, tss_limit);
        GDT[3] = tss_low;
        GDT[4] = tss_high;

        let gdtr = Gdtr {
            limit: (core::mem::size_of::<[u64; 5]>() - 1) as u16,
            base: core::ptr::addr_of!(GDT) as u64,
        };

        core::arch::asm!("lgdt [{0}]", in(reg) &gdtr, options(readonly, nostack, preserves_flags));
    }

    load_segments_and_tss();
    dump_gdtr();
    crate::serial_debugln!("[DBG] gdt::init done");
}
