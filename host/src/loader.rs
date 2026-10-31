//! Opening a plugin library and finding its entry point, with the
//! system's own loader (`dlopen`, `LoadLibraryW`): no crate needed.
//!
//! Libraries are never closed. A plugin's threads may still be running its
//! code when it is disabled, and closing the library under them would
//! crash the server; Paper doesn't unload a plugin's classes either.

use horizon_plugin_api::abi::{ENTRY_SYMBOL, EntryFn};
use std::ffi::c_void;
use std::path::Path;

/// The file extension plugin libraries have here.
pub const EXTENSION: &str = if cfg!(target_os = "windows") {
    "dll"
} else if cfg!(target_os = "macos") {
    "dylib"
} else {
    "so"
};

#[cfg(unix)]
mod sys {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const RTLD_NOW: c_int = 2;
    // RTLD_LOCAL is 0 everywhere this builds.
    #[cfg_attr(target_os = "linux", link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    fn last_error() -> String {
        // SAFETY: dlerror returns null or a NUL-terminated string.
        let e = unsafe { dlerror() };
        if e.is_null() {
            "unknown error".to_owned()
        } else {
            // SAFETY: as above.
            unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
        }
    }

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: a NUL-terminated path; running the library's initialisers
        // is what loading a plugin means.
        let h = unsafe { dlopen(c.as_ptr(), RTLD_NOW) };
        if h.is_null() {
            Err(last_error())
        } else {
            Ok(h)
        }
    }

    pub fn symbol(handle: *mut c_void, name: &[u8]) -> Result<*mut c_void, String> {
        // SAFETY: `name` is NUL-terminated (ENTRY_SYMBOL).
        let s = unsafe { dlsym(handle, name.as_ptr().cast()) };
        if s.is_null() {
            Err(last_error())
        } else {
            Ok(s)
        }
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated wide path.
        let h = unsafe { LoadLibraryW(wide.as_ptr()) };
        if h.is_null() {
            // SAFETY: no arguments.
            Err(format!("Windows error {}", unsafe { GetLastError() }))
        } else {
            Ok(h)
        }
    }

    pub fn symbol(handle: *mut c_void, name: &[u8]) -> Result<*mut c_void, String> {
        // SAFETY: `name` is NUL-terminated.
        let s = unsafe { GetProcAddress(handle, name.as_ptr()) };
        if s.is_null() {
            // SAFETY: no arguments.
            Err(format!("Windows error {}", unsafe { GetLastError() }))
        } else {
            Ok(s)
        }
    }
}

/// Opens `path` and finds its entry point.
pub fn open(path: &Path) -> Result<EntryFn, String> {
    let handle: *mut c_void = sys::open(path)?;
    let sym = sys::symbol(handle, ENTRY_SYMBOL).map_err(|e| {
        format!(
            "no `{}`: is it a Horizon plugin (horizon_plugin!)? {e}",
            String::from_utf8_lossy(&ENTRY_SYMBOL[..ENTRY_SYMBOL.len() - 1])
        )
    })?;
    // SAFETY: the symbol a plugin exports with `horizon_plugin!` is an EntryFn.
    Ok(unsafe { std::mem::transmute::<*mut c_void, EntryFn>(sym) })
}
