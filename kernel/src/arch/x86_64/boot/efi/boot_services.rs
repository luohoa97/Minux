//! UEFI Boot Services

use super::system_table::BootServices;

/// EFI Memory Type
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryType {
    ReservedMemoryType = 0,
    LoaderCode = 1,
    LoaderData = 2,
    BootServicesCode = 3,
    BootServicesData = 4,
    RuntimeServicesCode = 5,
    RuntimeServicesData = 6,
    ConventionalMemory = 7,
    UnusableMemory = 8,
    ACPIReclaimMemory = 9,
    ACPIMemoryNVS = 10,
    MemoryMappedIO = 11,
    MemoryMappedIOPortSpace = 12,
    PalCode = 13,
    PersistentMemory = 14,
}

/// EFI Memory Descriptor
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryDescriptor {
    pub memory_type: u32,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

/// Get memory map from boot services
pub fn get_memory_map(_boot_services: *const BootServices) -> &'static [MemoryDescriptor] {
    // Parse EFI memory map
    &[]
}
