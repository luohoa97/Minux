//! UEFI System Table structures

/// EFI System Table
#[repr(C)]
pub struct SystemTable {
    pub header: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: usize,
    pub con_in: usize,
    pub console_out_handle: usize,
    pub con_out: usize,
    pub standard_error_handle: usize,
    pub std_err: usize,
    pub runtime_services: *const RuntimeServices,
    pub boot_services: *const BootServices,
    pub number_of_table_entries: usize,
    pub configuration_table: *const ConfigurationTable,
}

/// EFI Table Header
#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

/// EFI Runtime Services
#[repr(C)]
pub struct RuntimeServices {
    pub header: TableHeader,
    // Runtime service function pointers
}

/// EFI Boot Services
#[repr(C)]
pub struct BootServices {
    pub header: TableHeader,
    // Boot service function pointers
}

/// EFI Configuration Table
#[repr(C)]
pub struct ConfigurationTable {
    pub vendor_guid: [u8; 16],
    pub vendor_table: *const u8,
}
