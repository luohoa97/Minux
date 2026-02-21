//! Memory management for minux microkernel
//!
//! In a microkernel, memory management is minimal:
//! - Address space isolation between tasks
//! - Basic page allocation
//! - Memory protection domains

pub mod heap;

/// Address space identifier
pub type AddressSpaceId = u32;

/// Memory protection domain
#[derive(Debug, Clone, Copy)]
pub struct AddressSpace {
    pub id: AddressSpaceId,
    pub page_table_root: u64,
}

impl AddressSpace {
    /// Create new address space
    pub const fn new(id: AddressSpaceId) -> Self {
        Self {
            id,
            page_table_root: 0,
        }
    }
}

use spin::Mutex;

/// Address space table
static ADDRESS_SPACES: Mutex<[Option<AddressSpace>; 32]> = Mutex::new([None; 32]);
static NEXT_AS_ID: Mutex<AddressSpaceId> = Mutex::new(1);

/// Initialize memory management
pub fn init() {
    // Create kernel address space (ID 0)
    let mut as0 = AddressSpace::new(0);
    as0.page_table_root = read_cr3_root();
    ADDRESS_SPACES.lock()[0] = Some(as0);
}

/// Create new address space
pub fn create_address_space() -> Result<AddressSpaceId, ()> {
    let mut spaces = ADDRESS_SPACES.lock();
    for slot in &mut *spaces {
        if slot.is_none() {
            let mut next_id = NEXT_AS_ID.lock();
            let id = *next_id;
            *next_id += 1;
            // Clone only kernel mappings (non-USER) from current CR3.
            // This preserves kernel reachability even if the kernel is linked
            // in low canonical addresses, while dropping active task user maps.
            let root = clone_table_level_kernel_only(read_cr3_root(), 4)?;
            let mut aspace = AddressSpace::new(id);
            aspace.page_table_root = root;
            *slot = Some(aspace);
            return Ok(id);
        }
    }
    Err(()) // No free slots
}

/// Get address space by ID (returns a copy)
pub fn get_address_space(id: AddressSpaceId) -> Option<AddressSpace> {
    let spaces = ADDRESS_SPACES.lock();
    spaces.get(id as usize)?.as_ref().copied()
}

/// Page mapping flags
#[derive(Debug, Clone, Copy)]
pub struct PageFlags {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub user_accessible: bool,
}

impl PageFlags {
    /// Create page flags from raw value
    pub fn from_raw(flags: u64) -> Self {
        Self {
            readable: (flags & 0x1) != 0,
            writable: (flags & 0x2) != 0,
            executable: (flags & 0x4) != 0,
            user_accessible: (flags & 0x8) != 0,
        }
    }
}

/// Page table entry
#[derive(Debug, Clone, Copy)]
pub struct PageTableEntry {
    pub virtual_addr: u64,
    pub physical_addr: u64,
    pub flags: PageFlags,
}

/// Simple page table for address space
pub struct PageTable {
    entries: [Option<PageTableEntry>; 256], // Simple fixed-size table
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [None; 256],
        }
    }
    
    /// Map a page
    pub fn map_page(&mut self, virtual_addr: u64, physical_addr: u64, flags: PageFlags) -> Result<(), ()> {
        // Find free slot
        for slot in &mut self.entries {
            if slot.is_none() {
                *slot = Some(PageTableEntry {
                    virtual_addr,
                    physical_addr,
                    flags,
                });
                return Ok(());
            }
        }
        Err(()) // No free slots
    }
    
    /// Unmap a page
    pub fn unmap_page(&mut self, virtual_addr: u64) -> Result<(), ()> {
        for slot in &mut self.entries {
            if let Some(entry) = slot {
                if entry.virtual_addr == virtual_addr {
                    *slot = None;
                    return Ok(());
                }
            }
        }
        Err(()) // Page not found
    }
    
    /// Translate virtual address to physical
    pub fn translate(&self, virtual_addr: u64) -> Option<u64> {
        for entry in &self.entries {
            if let Some(entry) = entry {
                if entry.virtual_addr == virtual_addr {
                    return Some(entry.physical_addr);
                }
            }
        }
        None
    }
}

/// Page tables for each address space (legacy bookkeeping, no longer used for translation)
static PAGE_TABLES: Mutex<[Option<PageTable>; 32]> = Mutex::new([const { None }; 32]);

/// L4-style static memory pool (NO heap allocation)
/// Pre-allocated physical memory for user-space programs
const STATIC_POOL_SIZE: usize = 16 * 1024 * 1024; // 16MB
#[repr(align(4096))]
struct StaticPool([u8; STATIC_POOL_SIZE]);
static mut STATIC_POOL: StaticPool = StaticPool([0; STATIC_POOL_SIZE]);
static STATIC_POOL_OFFSET: Mutex<usize> = Mutex::new(0);

/// Allocate a single page from static pool (L4-style, NO heap)
pub fn alloc_static_page() -> Result<u64, ()> {
    let mut offset = STATIC_POOL_OFFSET.lock();
    let page_size = 0x1000; // 4KB
    
    if *offset + page_size > STATIC_POOL_SIZE {
        return Err(()); // Out of memory
    }
    
    // SAFETY: allocation cursor is protected by STATIC_POOL_OFFSET mutex and
    // we only return raw addresses; no references to STATIC_POOL are created.
    let base = unsafe { core::ptr::addr_of_mut!(STATIC_POOL.0) as *mut u8 as u64 };
    let addr = base + *offset as u64;
    *offset += page_size;
    unsafe {
        core::ptr::write_bytes(addr as *mut u8, 0, page_size);
    }
    Ok(addr)
}

/// Allocate multiple pages from static pool (L4-style, NO heap)
pub fn alloc_static_pages(count: usize) -> Result<u64, ()> {
    let mut offset = STATIC_POOL_OFFSET.lock();
    let total_size = count * 0x1000; // 4KB pages
    
    if *offset + total_size > STATIC_POOL_SIZE {
        return Err(()); // Out of memory
    }
    
    // SAFETY: allocation cursor is protected by STATIC_POOL_OFFSET mutex and
    // we only return raw addresses; no references to STATIC_POOL are created.
    let base = unsafe { core::ptr::addr_of_mut!(STATIC_POOL.0) as *mut u8 as u64 };
    let addr = base + *offset as u64;
    *offset += total_size;
    Ok(addr)
}

/// Map page in address space
pub fn map_page(address_space_id: AddressSpaceId, virtual_addr: u64, physical_addr: u64, flags: u64) -> Result<(), ()> {
    if (address_space_id as usize) >= 32 {
        return Err(());
    }

    let root = {
        let spaces = ADDRESS_SPACES.lock();
        spaces
            .get(address_space_id as usize)
            .and_then(|s| *s)
            .map(|s| s.page_table_root)
            .ok_or(())?
    };

    map_page_4k(root, virtual_addr, physical_addr, flags)
}

/// Unmap page from address space
pub fn unmap_page(address_space_id: AddressSpaceId, virtual_addr: u64) -> Result<(), ()> {
    if (address_space_id as usize) >= 32 {
        return Err(());
    }
    let root = {
        let spaces = ADDRESS_SPACES.lock();
        spaces
            .get(address_space_id as usize)
            .and_then(|s| *s)
            .map(|s| s.page_table_root)
            .ok_or(())?
    };
    unmap_page_4k(root, virtual_addr)
}

/// Translate virtual address in address space
pub fn translate_address(address_space_id: AddressSpaceId, virtual_addr: u64) -> Option<u64> {
    if (address_space_id as usize) >= 32 {
        return None;
    }
    let root = {
        let spaces = ADDRESS_SPACES.lock();
        spaces
            .get(address_space_id as usize)
            .and_then(|s| *s)
            .map(|s| s.page_table_root)?
    };
    translate_4k(root, virtual_addr)
}

/// Activate an address space by loading CR3.
pub fn activate_address_space(address_space_id: AddressSpaceId) -> Result<(), ()> {
    let root = {
        let spaces = ADDRESS_SPACES.lock();
        spaces
            .get(address_space_id as usize)
            .and_then(|s| *s)
            .map(|s| s.page_table_root)
            .ok_or(())?
    };
    write_cr3_root(root);
    Ok(())
}

#[inline]
fn read_cr3_root() -> u64 {
    let cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nostack, preserves_flags));
    }
    cr3 & !0xfff
}

#[inline]
fn write_cr3_root(root: u64) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
    }
}

const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_WRITE: u64 = 1 << 1;
const PAGE_USER: u64 = 1 << 2;
const PAGE_NO_EXEC: u64 = 1u64 << 63;
const PAGE_HUGE: u64 = 1 << 7;
const PAGE_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;

#[inline]
fn idx_pml4(v: u64) -> usize { ((v >> 39) & 0x1ff) as usize }
#[inline]
fn idx_pdp(v: u64) -> usize { ((v >> 30) & 0x1ff) as usize }
#[inline]
fn idx_pd(v: u64) -> usize { ((v >> 21) & 0x1ff) as usize }
#[inline]
fn idx_pt(v: u64) -> usize { ((v >> 12) & 0x1ff) as usize }

fn flags_to_pte(flags: u64) -> u64 {
    let mut out = PAGE_PRESENT;
    if (flags & 0x2) != 0 {
        out |= PAGE_WRITE;
    }
    if (flags & 0x8) != 0 {
        out |= PAGE_USER;
    }
    // Do not set NX unless EFER.NXE is enabled; otherwise bit63 is reserved
    // and first user stack access can fault with RSVD=1.
    let _ = flags;
    out
}

fn clone_table_level_kernel_only(src_phys: u64, level: u8) -> Result<u64, ()> {
    let dst_phys = alloc_static_page()?;
    let src = src_phys as *const u64;
    let dst = dst_phys as *mut u64;
    let mut has_present = false;

    for i in 0..512usize {
        let entry = unsafe { core::ptr::read_volatile(src.add(i)) };
        let new_entry = if (entry & PAGE_PRESENT) == 0 {
            0
        } else if level == 1 || (entry & PAGE_HUGE) != 0 {
            // Leaf mapping (4K at PT level, or huge page at upper levels).
            if (entry & PAGE_USER) != 0 {
                0
            } else {
                entry
            }
        } else {
            let child_src = entry & PAGE_ADDR_MASK;
            let child_dst = clone_table_level_kernel_only(child_src, level - 1)?;
            if table_has_present(child_dst) {
                // Keep non-leaf as supervisor-only in cloned space.
                ((entry & !PAGE_ADDR_MASK) & !PAGE_USER) | (child_dst & PAGE_ADDR_MASK)
            } else {
                0
            }
        };
        if (new_entry & PAGE_PRESENT) != 0 {
            has_present = true;
        }
        unsafe {
            core::ptr::write_volatile(dst.add(i), new_entry);
        }
    }
    if !has_present {
        return Ok(dst_phys);
    }
    Ok(dst_phys)
}

fn table_has_present(table_phys: u64) -> bool {
    let table = table_phys as *const u64;
    for i in 0..512usize {
        let entry = unsafe { core::ptr::read_volatile(table.add(i)) };
        if (entry & PAGE_PRESENT) != 0 {
            return true;
        }
    }
    false
}

fn ensure_next_table(table_phys: u64, index: usize, user: bool) -> Result<u64, ()> {
    let entry_ptr = (table_phys as *mut u64).wrapping_add(index);
    let mut entry = unsafe { core::ptr::read_volatile(entry_ptr) };
    if (entry & PAGE_PRESENT) == 0 {
        let new_page = alloc_static_page()?;
        let mut new_entry = (new_page & PAGE_ADDR_MASK) | PAGE_PRESENT | PAGE_WRITE;
        if user {
            new_entry |= PAGE_USER;
        }
        unsafe { core::ptr::write_volatile(entry_ptr, new_entry) };
        entry = new_entry;
    } else if user && (entry & PAGE_USER) == 0 {
        entry |= PAGE_USER;
        unsafe { core::ptr::write_volatile(entry_ptr, entry) };
    }
    Ok(entry & PAGE_ADDR_MASK)
}

fn map_page_4k(root: u64, virtual_addr: u64, physical_addr: u64, flags: u64) -> Result<(), ()> {
    let user = (flags & 0x8) != 0;
    let pml4 = root;
    let pdp = ensure_next_table(pml4, idx_pml4(virtual_addr), user)?;
    let pd = ensure_next_table(pdp, idx_pdp(virtual_addr), user)?;
    let pt = ensure_next_table(pd, idx_pd(virtual_addr), user)?;

    let pte_ptr = (pt as *mut u64).wrapping_add(idx_pt(virtual_addr));
    let pte = (physical_addr & PAGE_ADDR_MASK) | flags_to_pte(flags);
    unsafe {
        core::ptr::write_volatile(pte_ptr, pte);
        core::arch::asm!("invlpg [{}]", in(reg) virtual_addr, options(nostack, preserves_flags));
    }
    Ok(())
}

fn unmap_page_4k(root: u64, virtual_addr: u64) -> Result<(), ()> {
    let pml4e = unsafe { core::ptr::read_volatile((root as *const u64).add(idx_pml4(virtual_addr))) };
    if (pml4e & PAGE_PRESENT) == 0 {
        return Err(());
    }
    let pdp = pml4e & PAGE_ADDR_MASK;
    let pdpe = unsafe { core::ptr::read_volatile((pdp as *const u64).add(idx_pdp(virtual_addr))) };
    if (pdpe & PAGE_PRESENT) == 0 {
        return Err(());
    }
    let pd = pdpe & PAGE_ADDR_MASK;
    let pde = unsafe { core::ptr::read_volatile((pd as *const u64).add(idx_pd(virtual_addr))) };
    if (pde & PAGE_PRESENT) == 0 {
        return Err(());
    }
    let pt = pde & PAGE_ADDR_MASK;
    let pte_ptr = (pt as *mut u64).wrapping_add(idx_pt(virtual_addr));
    unsafe {
        core::ptr::write_volatile(pte_ptr, 0);
        core::arch::asm!("invlpg [{}]", in(reg) virtual_addr, options(nostack, preserves_flags));
    }
    Ok(())
}

fn translate_4k(root: u64, virtual_addr: u64) -> Option<u64> {
    let pml4e = unsafe { core::ptr::read_volatile((root as *const u64).add(idx_pml4(virtual_addr))) };
    if (pml4e & PAGE_PRESENT) == 0 {
        return None;
    }
    let pdp = pml4e & PAGE_ADDR_MASK;
    let pdpe = unsafe { core::ptr::read_volatile((pdp as *const u64).add(idx_pdp(virtual_addr))) };
    if (pdpe & PAGE_PRESENT) == 0 {
        return None;
    }
    let pd = pdpe & PAGE_ADDR_MASK;
    let pde = unsafe { core::ptr::read_volatile((pd as *const u64).add(idx_pd(virtual_addr))) };
    if (pde & PAGE_PRESENT) == 0 {
        return None;
    }
    let pt = pde & PAGE_ADDR_MASK;
    let pte = unsafe { core::ptr::read_volatile((pt as *const u64).add(idx_pt(virtual_addr))) };
    if (pte & PAGE_PRESENT) == 0 {
        return None;
    }
    Some((pte & PAGE_ADDR_MASK) | (virtual_addr & 0xfff))
}
