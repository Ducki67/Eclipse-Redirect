extern crate alloc;

mod hooks;
pub mod opts;
mod patchfinder;
mod pe;
mod redirection;
mod unreal;
mod url;

static mut THREAD_HANDLE: *mut core::ffi::c_void = core::ptr::null_mut();

unsafe extern "system" fn entry(_param: *mut core::ffi::c_void) -> u32 {
    hooks::init_hooks();
    0
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: *mut core::ffi::c_void,
    reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    if reason == 1 {
        if opts::B_MANUAL_MAPPING {
            hooks::init_hooks();
        } else {
            let mut thread_id: u32 = 0;
            THREAD_HANDLE = hooks::create_thread(entry, &mut thread_id);
        }
    }
    1
}
