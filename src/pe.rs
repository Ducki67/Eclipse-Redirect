use core::ptr;

#[repr(C, packed)]
struct DosHeader {
    e_magic: u16,
    _fields: [u8; 58],
    e_lfanew: i32,
}

#[repr(C, packed)]
struct CoffHeader {
    machine: u16,
    number_of_sections: u16,
    _timestamp: u32,
    _pointer_to_symbol_table: u32,
    _number_of_symbols: u32,
    size_of_optional_header: u16,
    _characteristics: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    _relocations: u32,
    _line_numbers: u32,
    _number_of_relocations: u16,
    _number_of_line_numbers: u16,
    pub characteristics: u32,
}

#[repr(C)]
struct MemoryBasicInformation {
    base_address: usize,
    allocation_base: usize,
    allocation_protect: u32,
    _partition_id: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    type_: u32,
}

extern "system" {
    fn VirtualQuery(
        lpaddress: *const core::ffi::c_void,
        lpbuffer: *mut MemoryBasicInformation,
        dwlength: usize,
    ) -> usize;
}

pub fn is_readable(addr: u64, len: usize) -> bool {
    if addr == 0 || len == 0 {
        return false;
    }
    unsafe {
        let mut mbi: MemoryBasicInformation = core::mem::zeroed();
        let result = VirtualQuery(
            addr as *const core::ffi::c_void,
            &mut mbi as *mut MemoryBasicInformation,
            core::mem::size_of::<MemoryBasicInformation>(),
        );
        if result == 0 || mbi.state != 0x1000 {
            return false;
        }
        if mbi.protect & 0x100 != 0 {
            return false;
        }
        if mbi.protect & 0xEE == 0 {
            return false;
        }
        let region_end = mbi.base_address as u64 + mbi.region_size as u64;
        addr >= mbi.base_address as u64 && addr.saturating_add(len as u64) <= region_end
    }
}

pub struct ImageInfo {
    pub base: u64,
}

impl ImageInfo {
    pub fn get_image_base() -> u64 {
        unsafe {
            let peb_ptr: *const u8;
            core::arch::asm!("mov {0}, gs:[0x60]", out(reg) peb_ptr);
            let image_base_ptr = peb_ptr.add(0x10) as *const u64;
            ptr::read_unaligned(image_base_ptr)
        }
    }

    pub fn new() -> Self {
        Self {
            base: Self::get_image_base(),
        }
    }

    pub fn with_base(base: u64) -> Self {
        Self { base }
    }

    fn dos_header(&self) -> &DosHeader {
        unsafe { &*(self.base as *const DosHeader) }
    }

    fn coff_header(&self) -> &CoffHeader {
        let dos = self.dos_header();
        unsafe { &*((self.base + dos.e_lfanew as u64 + 4) as *const CoffHeader) }
    }

    fn first_section(&self) -> *const SectionHeader {
        let dos = self.dos_header();
        let coff = self.coff_header();
        let header_end = (self.base as usize)
            + dos.e_lfanew as usize
            + 4
            + core::mem::size_of::<CoffHeader>()
            + coff.size_of_optional_header as usize;
        header_end as *const SectionHeader
    }

    pub fn get_section(&self, name: &str) -> Option<SectionHeader> {
        let coff = self.coff_header();
        let sections = self.first_section();
        let name_bytes = name.as_bytes();

        for i in 0..coff.number_of_sections as usize {
            let section = unsafe { &*sections.add(i) };
            let mut matches = true;
            for (j, &b) in name_bytes.iter().enumerate() {
                if j >= 8 || section.name[j] != b {
                    matches = false;
                    break;
                }
            }
            if matches && name_bytes.len() <= 8 {
                return Some(*section);
            }
        }
        None
    }

    fn query_region_size(addr: u64) -> usize {
        unsafe {
            let mut mbi: MemoryBasicInformation = core::mem::zeroed();
            let result = VirtualQuery(
                addr as *const core::ffi::c_void,
                &mut mbi as *mut MemoryBasicInformation,
                core::mem::size_of::<MemoryBasicInformation>(),
            );
            if result == 0 {
                return 0;
            }
            if mbi.state != 0x1000 {
                return 0;
            }
            let region_start = mbi.base_address as u64;
            let region_end = region_start + mbi.region_size as u64;
            if addr >= region_start && addr < region_end {
                (region_end - addr) as usize
            } else {
                0
            }
        }
    }

    pub fn text_section(&self) -> Option<(u64, u32)> {
        let s = self.get_section(".text")?;
        let addr = self.base + s.virtual_address as u64;
        let pe_size = s.virtual_size as usize;
        let committed = Self::query_region_size(addr);
        let size = if committed > 0 && committed < pe_size {
            committed as u32
        } else {
            s.virtual_size
        };
        Some((addr, size))
    }

    pub fn rdata_section(&self) -> Option<(u64, u32)> {
        let s = self.get_section(".rdata")?;
        let addr = self.base + s.virtual_address as u64;
        let pe_size = s.virtual_size as usize;
        let committed = Self::query_region_size(addr);
        let size = if committed > 0 && committed < pe_size {
            committed as u32
        } else {
            s.virtual_size
        };
        Some((addr, size))
    }
}
