use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::opts;
use crate::patchfinder;
use crate::pe::{self, ImageInfo};
use crate::redirection;
use crate::unreal::{
    FCurlHttpRequest, FMemory, FString, F_MEMORY_REALLOC, PROCESS_REQUEST_OG,
    PROCESS_REQUEST_OG_EOS, SET_URL_IDX,
};
use crate::url::ParsedUrl;

extern "system" {
    fn GetCommandLineW() -> *const u16;
    fn SetConsoleTitleA(lpconsoletitle: *const u8);
    fn WriteFile(
        hfile: *mut core::ffi::c_void,
        lpbuffer: *const u8,
        nnumberofbytestowrite: u32,
        lpnumberofbyteswritten: *mut u32,
        lpoverlapped: *mut core::ffi::c_void,
    ) -> i32;
    fn WriteConsoleA(
        hconsoleoutput: *mut core::ffi::c_void,
        lpbuffer: *const core::ffi::c_void,
        nnumberofcharstowrite: u32,
        lpnumberofcharswritten: *mut u32,
        lpreserved: *mut core::ffi::c_void,
    ) -> i32;
    fn GetStdHandle(nstdhandle: u32) -> *mut core::ffi::c_void;
    fn CreateFileA(
        lpfilename: *const u8,
        dwdesiredaccess: u32,
        dwsharemode: u32,
        lpsecurityattributes: *mut core::ffi::c_void,
        dwcreationdisposition: u32,
        dwflagsandattributes: u32,
        htemplatefile: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn GetTempPathA(nbufferlength: u32, lpbuffer: *mut u8) -> u32;
    fn OutputDebugStringA(lpoutputstring: *const u8);
}

type LpThreadStartRoutine = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;

extern "system" {
    fn CreateThread(
        lpthreadattributes: *const core::ffi::c_void,
        dwstacksize: usize,
        lpstartaddress: LpThreadStartRoutine,
        lpparameter: *mut core::ffi::c_void,
        dwcreationflags: u32,
        lpthreadid: *mut u32,
    ) -> *mut core::ffi::c_void;
}

pub unsafe fn create_thread(
    func: LpThreadStartRoutine,
    thread_id: *mut u32,
) -> *mut core::ffi::c_void {
    CreateThread(
        core::ptr::null(),
        0,
        func,
        core::ptr::null_mut(),
        0,
        thread_id,
    )
}

static mut BACKEND_WIDE: *mut u16 = core::ptr::null_mut();
static mut LOG_FILE: *mut core::ffi::c_void = core::ptr::null_mut();
static mut CONSOLE_HANDLE: *mut core::ffi::c_void = core::ptr::null_mut();
static LOG_INIT: AtomicBool = AtomicBool::new(false);

fn init_log() {
    if LOG_INIT.swap(true, Ordering::Relaxed) {
        return;
    }

    unsafe {
        let mut path_buf = [0u8; 512];
        let len = GetTempPathA(512, path_buf.as_mut_ptr());
        if len == 0 || len as usize > path_buf.len() {
            return;
        }
        let log_name = b"eclipse_redirect.log\0";
        let mut full_path = alloc::vec![0u8; len as usize + log_name.len()];
        full_path[..len as usize]
            .copy_from_slice(core::slice::from_raw_parts(path_buf.as_ptr(), len as usize));
        full_path[len as usize..].copy_from_slice(log_name);

        LOG_FILE = CreateFileA(
            full_path.as_ptr(),
            0x40000000,
            1,
            core::ptr::null_mut(),
            2,
            0x80,
            core::ptr::null_mut(),
        );
    }
}

pub fn log_status(msg: &str) {
    unsafe {
        let full = alloc::format!("[Eclipse] {}\0", msg);
        OutputDebugStringA(full.as_ptr());

        if !CONSOLE_HANDLE.is_null() {
            let line = alloc::format!("{}\n", msg);
            let mut written: u32 = 0;
            WriteConsoleA(
                CONSOLE_HANDLE,
                line.as_ptr() as *const core::ffi::c_void,
                line.len() as u32,
                &mut written,
                core::ptr::null_mut(),
            );
        }

        if LOG_FILE as isize != -1 && !LOG_FILE.is_null() {
            let line = alloc::format!("{}\r\n", msg);
            let mut written: u32 = 0;
            WriteFile(
                LOG_FILE,
                line.as_ptr(),
                line.len() as u32,
                &mut written,
                core::ptr::null_mut(),
            );
        }
    }
}

fn set_backend_url(url: &str) -> bool {
    let fstr = FString::to_wide(url);
    if fstr.is_empty() {
        return false;
    }
    unsafe {
        if !BACKEND_WIDE.is_null() {
            FMemory::free(BACKEND_WIDE as *mut u8);
        }
        BACKEND_WIDE = fstr.raw();
        core::mem::forget(fstr);
    }
    true
}

fn make_backend_fstr() -> FString {
    unsafe {
        if BACKEND_WIDE.is_null() {
            return FString::new();
        }
        FString::from_wide(BACKEND_WIDE)
    }
}

extern "system" fn process_request_hook(this: *mut FCurlHttpRequest) -> bool {
    unsafe {
        redirect_request(this, false);
        let og: extern "system" fn(*mut FCurlHttpRequest) -> bool =
            core::mem::transmute(PROCESS_REQUEST_OG.load(Ordering::Relaxed));
        og(this)
    }
}

extern "system" fn process_request_hook_eos(this: *mut FCurlHttpRequest) -> bool {
    unsafe {
        redirect_request(this, true);
        let og: extern "system" fn(*mut FCurlHttpRequest) -> bool =
            core::mem::transmute(PROCESS_REQUEST_OG_EOS.load(Ordering::Relaxed));
        og(this)
    }
}

unsafe fn redirect_request(request: *mut FCurlHttpRequest, is_eos: bool) {
    if request.is_null() {
        return;
    }

    let req = &mut *request;

    if !is_eos && SET_URL_IDX.load(Ordering::Relaxed) == 0 {
        req.initialize_url_index();
        log_status(&alloc::format!(
            "SetURL index: {}",
            SET_URL_IDX.load(Ordering::Relaxed)
        ));
    }

    let tag = if is_eos { "EOS" } else { "Main" };

    let url = req.get_url();
    if url.is_empty() {
        if opts::LOG_REQUESTS {
            log_status(&alloc::format!("{}: empty URL", tag));
        }
        return;
    }

    if opts::LOG_REQUESTS {
        log_status(&alloc::format!("{}: {}", tag, url.to_utf8()));
    }

    let mut parsed = ParsedUrl::parse(&url);
    drop(url);

    if redirection::should_redirect(&parsed) {
        let backend = make_backend_fstr();
        parsed.set_host(&backend);
        drop(backend);

        let new_url = parsed.get_url();
        if !new_url.is_empty() {
            if opts::LOG_REQUESTS {
                log_status(&alloc::format!("  -> {}", new_url.to_utf8()));
            }
            req.set_url(&new_url, is_eos);
        } else if opts::LOG_REQUESTS {
            log_status("  -> rebuild FAILED, left untouched");
        }
        drop(new_url);
    }

    drop(parsed);
}

fn find_and_patch_exit_functions(image: &ImageInfo) {
    let (text_start, text_size) = match image.text_section() {
        Some(s) => s,
        None => return,
    };

    const PUSH_WIDGET_PATTERNS: [patchfinder::ParsedPattern; 4] = [
        patchfinder::parse_pattern("48 89 5C 24 ? 48 89 6C 24 ? 48 89 74 24 ? 57 48 83 EC 30 48 8B E9 49 8B D9 48 8D 0D ? ? ? ? 49 8B F8 48 8B F2 E8 ? ? ? ? 4C 8B CF 48 89 5C 24 ? 4C 8B C6 48 8B D5 48 8B 48 78"),
        patchfinder::parse_pattern("48 8B C4 4C 89 40 18 48 89 50 10 48 89 48 08 55 53 56 57 41 54 41 55 41 56 41 57 48 8D 68 B8 48 81 EC ? ? ? ? 65 48 8B 04 25"),
        patchfinder::parse_pattern("48 8B C4 48 89 58 ? 48 89 70 ? 48 89 78 ? 55 41 56 41 57 48 8D 68 A1 48 81 EC ? ? ? ? 65 48 8B 04 25 ? ? ? ? 48 8B F9 B9 ? ? ? ?"),
        patchfinder::parse_pattern("48 89 5C 24 ? 48 89 74 24 ? 55 57 41 54 41 56 41 57 48 8D 6C 24 ? 48 81 EC ? ? ? ? 49 8B D9 49 8B F8 4C 8B E2 4C 8B F1"),
    ];

    let mut push_widget = 0u64;
    for p in PUSH_WIDGET_PATTERNS.iter() {
        push_widget = patchfinder::find_pattern(text_start, text_size, p);
        if push_widget != 0 {
            break;
        }
    }

    if push_widget == 0 {
        log_status("PushWidget not found, skipping exit patches");
        return;
    }
    log_status(&alloc::format!("PushWidget @ 0x{:X}", push_widget));

    const EXIT_PATTERNS: [patchfinder::ParsedPattern; 3] = [
        patchfinder::parse_pattern("48 89 5C 24 ? 57 48 83 EC 40 41 B9 ? ? ? ? 0F B6 F9 44 38 0D ? ? ? ? 0F B6 DA 72 24 89 5C 24 30 48 8D 05 ? ? ? ? 89 7C 24 28 4C 8D 05 ? ? ? ? 33 D2 48 89 44 24 ? 33 C9 E8 ? ? ? ?"),
        patchfinder::parse_pattern("48 8B C4 48 89 58 18 88 50 10 88 48 08 57 48 83 EC 30"),
        patchfinder::parse_pattern("4C 8B DC 49 89 5B 08 49 89 6B 10 49 89 73 18 49 89 7B 20 41 56 48 83 EC 30 80 3D ? ? ? ? ? 49 8B"),
    ];

    let mut exit_addr = 0u64;
    for p in EXIT_PATTERNS.iter() {
        let addr = patchfinder::find_pattern(text_start, text_size, p);
        if addr != 0 {
            exit_addr = addr;
            break;
        }
    }

    if exit_addr != 0 {
        unsafe { patch_first_byte(exit_addr) };
        log_status("exit patched");
    }

    const ENV_PATTERNS: [patchfinder::ParsedPattern; 7] = [
        patchfinder::parse_pattern("4C 8B DC 55 49 8D AB ? ? ? ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 ? ? ? ? 49 89 73 F0 49 89 7B E8 48 8B F9 4D 89 63 E0 4D 8B E0 4D 89 6B D8"),
        patchfinder::parse_pattern("48 89 5C 24 ? 55 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 ? 41 0F B6 D8 48 89 55 ? 88 5C 24 ?"),
        patchfinder::parse_pattern("48 89 5C 24 ? 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 ? ? ? ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 ? ? ? ? 80 B9 ? ? ? ? ? 48 8B DA 48 8B F1"),
        patchfinder::parse_pattern("48 89 5C 24 ? 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 ? ? ? ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 ? ? ? ? ? 0F B6 ? 44 88 44 24 ?"),
        patchfinder::parse_pattern("48 89 5C 24 ? 55 56 57 41 54 41 55 41 56 41 57 48 8D 6C 24 ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 ? 45 0F B6 F8"),
        patchfinder::parse_pattern("40 55 53 56 57 41 54 41 56 41 57 48 8D AC 24 ? ? ? ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 ? ? ? ? ? 0F B6 ?"),
        patchfinder::parse_pattern("4C 8B DC 55 49 8D AB ? ? ? ? 48 81 EC ? ? ? ? 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 ? ? ? ?"),
    ];

    let mut env_addr = 0u64;
    for p in ENV_PATTERNS.iter() {
        let addr = patchfinder::find_pattern(text_start, text_size, p);
        if addr != 0 {
            env_addr = addr;
            break;
        }
    }

    if env_addr != 0 {
        unsafe { patch_first_byte(env_addr) };
        log_status("security bypass patched");
    }
}

const KNOWN_PROCESS_REQUEST_PROLOGUE: [u8; 23] = [
    0x48, 0x89, 0x5C, 0x24, 0x20, 0x55, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57,
    0x48, 0x8B, 0xEC, 0x48, 0x83, 0xEC, 0x40,
];

fn known_vtable_entry(image: &ImageInfo, text_start: u64, text_size: u32) -> u64 {
    if opts::PROCESS_REQUEST_VTABLE_RVA == 0 {
        return 0;
    }

    let slot = image.base + opts::PROCESS_REQUEST_VTABLE_RVA;
    if !pe::is_readable(slot, 8) {
        return 0;
    }

    let target = unsafe { *(slot as *const u64) };
    if target < text_start || target >= text_start + text_size as u64 {
        return 0;
    }

    if !pe::is_readable(target, KNOWN_PROCESS_REQUEST_PROLOGUE.len()) {
        return 0;
    }

    if !patchfinder::check_bytes_at(target, 0, &KNOWN_PROCESS_REQUEST_PROLOGUE, false) {
        return 0;
    }

    slot
}

pub fn initialize_for_module(module_base: u64, is_eos: bool) -> bool {
    let tag = if is_eos { "EOS" } else { "Main" };
    let image = ImageInfo::with_base(module_base);

    let (text_start, text_size) = match image.text_section() {
        Some(s) => s,
        None => {
            log_status(&alloc::format!("{}: no .text section", tag));
            return false;
        }
    };

    let (rdata_start, rdata_size) = match image.rdata_section() {
        Some(s) => s,
        None => {
            log_status(&alloc::format!("{}: no .rdata section", tag));
            return false;
        }
    };

    log_status(&alloc::format!(
        "{}: .text 0x{:X} size 0x{:X} | .rdata 0x{:X} size 0x{:X}",
        tag,
        text_start,
        text_size,
        rdata_start,
        rdata_size
    ));

    let mut vtable_entry = 0u64;
    let mut proc_req = 0u64;

    if !is_eos {
        vtable_entry = known_vtable_entry(&image, text_start, text_size);
        if vtable_entry != 0 {
            proc_req = unsafe { *(vtable_entry as *const u64) };
            log_status(&alloc::format!(
                "{}: known vtable entry 0x{:X} -> ProcessRequest 0x{:X}",
                tag,
                vtable_entry,
                proc_req
            ));
        }
    }

    if vtable_entry == 0 {
        let str_ref = {
            let wide_pattern: alloc::vec::Vec<u16> = "STAT_FCurlHttpRequest_ProcessRequest"
                .encode_utf16()
                .chain(core::iter::once(0))
                .collect();
            let r = patchfinder::find_string_ref_wide(&image, &wide_pattern);
            if r != 0 {
                log_status(&alloc::format!("{}: found STAT str @ 0x{:X}", tag, r));
                r
            } else {
                let alt_pattern: alloc::vec::Vec<u16> =
                    "%p: request (easy handle:%p) has been added to threaded queue for processing"
                        .encode_utf16()
                        .chain(core::iter::once(0))
                        .collect();
                let r = patchfinder::find_string_ref_wide(&image, &alt_pattern);
                if r != 0 {
                    log_status(&alloc::format!("{}: found alt str @ 0x{:X}", tag, r));
                    r
                } else {
                    let narrow = b"STAT_FCurlHttpRequest_ProcessRequest\0";
                    let r = patchfinder::find_string_ref_ascii(&image, narrow);
                    if r != 0 {
                        log_status(&alloc::format!("{}: found narrow str @ 0x{:X}", tag, r));
                    }
                    r
                }
            }
        };

        if str_ref == 0 {
            log_status(&alloc::format!("{}: no string ref found!", tag));
            return false;
        }

        proc_req = patchfinder::find_function_prologue(str_ref, is_eos);
        if proc_req == 0 {
            log_status(&alloc::format!("{}: no prologue found!", tag));
            return false;
        }
        log_status(&alloc::format!("{}: ProcessRequest @ 0x{:X}", tag, proc_req));

        vtable_entry = patchfinder::find_vtable_entry(rdata_start, rdata_size, proc_req);
        if vtable_entry == 0 {
            log_status(&alloc::format!("{}: no vtable entry found!", tag));
            return false;
        }
        log_status(&alloc::format!("{}: VTable entry @ 0x{:X}", tag, vtable_entry));
    }

    let hook_fn = if is_eos {
        process_request_hook_eos as *const () as usize
    } else {
        process_request_hook as *const () as usize
    };

    unsafe {
        if is_eos {
            PROCESS_REQUEST_OG_EOS.store(proc_req, Ordering::Relaxed);
        } else {
            PROCESS_REQUEST_OG.store(proc_req, Ordering::Relaxed);
        }

        let old_prot = apply_hook(vtable_entry, hook_fn as u64);
        restore_prot(vtable_entry, old_prot);
    }

    log_status(&alloc::format!("{}: HOOKED!", tag));
    true
}

unsafe fn apply_hook(vtable_entry: u64, hook_fn: u64) -> u32 {
    let mut old_prot: u32 = 0;
    windows_sys::Win32::System::Memory::VirtualProtect(
        vtable_entry as *mut _,
        core::mem::size_of::<u64>(),
        windows_sys::Win32::System::Memory::PAGE_EXECUTE_READWRITE,
        &mut old_prot,
    );
    *(vtable_entry as *mut u64) = hook_fn;
    old_prot
}

unsafe fn restore_prot(vtable_entry: u64, old_prot: u32) {
    let mut _dummy: u32 = 0;
    windows_sys::Win32::System::Memory::VirtualProtect(
        vtable_entry as *mut _,
        core::mem::size_of::<u64>(),
        old_prot,
        &mut _dummy,
    );
}

unsafe fn patch_first_byte(addr: u64) {
    let mut old_prot: u32 = 0;
    windows_sys::Win32::System::Memory::VirtualProtect(
        addr as *mut _,
        1,
        windows_sys::Win32::System::Memory::PAGE_EXECUTE_READWRITE,
        &mut old_prot,
    );
    *(addr as *mut u8) = 0xC3;
    let mut _dummy: u32 = 0;
    windows_sys::Win32::System::Memory::VirtualProtect(addr as *mut _, 1, old_prot, &mut _dummy);
}

pub fn init_hooks() {
    let mut backend_url = alloc::string::String::from(opts::BACKEND);

    if opts::B_USE_ARG_PARAMS {
        if let Some(cmd) = get_cmd_line() {
            for arg in cmd.split_whitespace() {
                if let Some(val) = arg.strip_prefix("-backend=") {
                    backend_url = alloc::string::String::from(val);
                }
                if let Some(val) = arg.strip_prefix("-bConsole=") {
                    if val.eq_ignore_ascii_case("true") {
                        alloc_console();
                    }
                }
            }
        }
    } else if opts::CONSOLE {
        alloc_console();
    }

    init_log();
    log_status("init start");

    let image = ImageInfo::new();
    log_status(&alloc::format!("image base: 0x{:X}", image.base));

    if FMemory::find_realloc(&image) == 0 {
        log_status("FMemory::Realloc NOT FOUND - FString ops will fail");
    } else {
        log_status(&alloc::format!(
            "FMemory::Realloc @ 0x{:X}",
            F_MEMORY_REALLOC.load(Ordering::Relaxed)
        ));
    }

    if set_backend_url(&backend_url) {
        log_status(&alloc::format!("backend: {}", backend_url));
    } else {
        log_status(&alloc::format!(
            "backend ALLOC FAILED: {} - requests will NOT be redirected",
            backend_url
        ));
    }

    log_status("hooking main module...");
    let mut attempts = 0;
    while !initialize_for_module(image.base, false) {
        attempts += 1;
        if attempts >= 3 {
            log_status("main module hook FAILED");
            break;
        }
    }

    log_status("loading EOS SDK...");
    let eos_module = load_eos_module();
    if eos_module != 0 {
        log_status(&alloc::format!("EOS base: 0x{:X}", eos_module));
        initialize_for_module(eos_module, true);
    } else {
        log_status("EOS SDK not found");
    }

    if opts::B_HAS_PUSH_WIDGET {
        if eos_module != 0 {
            log_status("patching exit functions...");
            find_and_patch_exit_functions(&image);
        } else {
            log_status("no EOS SDK, skipping exit patches");
        }
    }

    log_status("init done - hook active!");
}

pub fn alloc_console() {
    unsafe {
        windows_sys::Win32::System::Console::AllocConsole();

        let handle = CreateFileA(
            b"CONOUT$\0".as_ptr(),
            0xC0000000,
            3,
            core::ptr::null_mut(),
            3,
            0,
            core::ptr::null_mut(),
        );

        CONSOLE_HANDLE = if handle as isize != -1 && !handle.is_null() {
            handle
        } else {
            GetStdHandle(0xFFFFFFF5)
        };

        SetConsoleTitleA(
            b"Eclipse Redirect - https://github.com/Ducki67/Eclipse-Redirect\0".as_ptr(),
        );
    }
}

fn load_eos_module() -> u64 {
    let name = b"EOSSDK-Win64-Shipping\0";
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::LoadLibraryA(name.as_ptr()) };
    module as u64
}

pub fn get_cmd_line() -> Option<alloc::string::String> {
    unsafe {
        let cmd_line = GetCommandLineW();
        if cmd_line.is_null() {
            return None;
        }
        let mut len = 0;
        while *cmd_line.add(len) != 0 {
            len += 1;
        }
        let cmd = core::slice::from_raw_parts(cmd_line, len);
        Some(alloc::string::String::from_utf16_lossy(cmd))
    }
}
