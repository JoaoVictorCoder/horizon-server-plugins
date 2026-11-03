//! The C ABI between the server and a plugin library. Only these `repr(C)`
//! types and JSON text cross it, so a plugin built with one Rust (or any
//! language that can export a C function) loads in a server built with
//! another. Each side frees only the buffers it allocated, through its own
//! `free`.
//!
//! A plugin library exports [`ENTRY_SYMBOL`], an [`EntryFn`]. The server
//! calls it once with a [`HostApi`] it never frees, and talks to the
//! plugin through the [`PluginApi`] it returns (always from the server's
//! thread):
//! `handle(kind, payload)` with a JSON payload, answered with JSON. The
//! plugin calls back with `HostApi::call(method, args)`, also JSON. Both
//! answer `{"error": "..."}` on failure.

use std::ffi::c_void;

/// The ABI's version. A server loads only plugins of its own version.
pub const ABI_VERSION: u32 = 1;

/// The entry point a plugin library exports, NUL-terminated for `dlsym`.
pub const ENTRY_SYMBOL: &[u8] = b"horizon_plugin_entry_v1\0";

/// Borrowed UTF-8 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl RawStr {
    pub fn new(s: &str) -> Self {
        RawStr {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// # Safety
    /// `ptr` must point at `len` bytes that live as long as `'a`.
    pub unsafe fn as_str<'a>(self) -> &'a str {
        if self.ptr.is_null() || self.len == 0 {
            return "";
        }
        // SAFETY: the caller vouches for the bytes; invalid UTF-8 reads as
        // empty rather than being trusted.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes).unwrap_or("")
    }
}

/// Owned bytes, freed by the side that allocated them.
#[repr(C)]
pub struct RawBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl RawBuf {
    /// Hands `s` over; free it with [`free_buf`] on the same side.
    pub fn from_string(s: String) -> Self {
        let mut s = std::mem::ManuallyDrop::new(s.into_bytes());
        RawBuf {
            ptr: s.as_mut_ptr(),
            len: s.len(),
            cap: s.capacity(),
        }
    }

    pub fn empty() -> Self {
        RawBuf {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    /// The bytes as text, copied.
    ///
    /// # Safety
    /// The buffer must be live: not yet freed.
    pub unsafe fn to_string_lossy(&self) -> String {
        if self.ptr.is_null() {
            return String::new();
        }
        // SAFETY: a live buffer holds `len` initialised bytes.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Frees a buffer this side made with [`RawBuf::from_string`]. Each side
/// passes its own copy of this function, so the allocator that made a
/// buffer frees it.
///
/// # Safety
/// `buf` must come from `RawBuf::from_string` in this same library, once.
pub unsafe extern "C" fn free_buf(buf: RawBuf) {
    if !buf.ptr.is_null() {
        // SAFETY: rebuilt from the parts `from_string` gave away.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

/// What the server gives a plugin. The server never frees it (a plugin's
/// threads may outlive the plugin), and `call` may come from any thread:
/// the server answers calls from other threads on its own, waiting, or
/// with an error once the plugin is disabled.
#[repr(C)]
pub struct HostApi {
    pub abi: u32,
    /// The server's own state, passed back to `call`.
    pub ctx: *mut c_void,
    /// This plugin's number on the server when it was started; passed
    /// back to `call` (the server may go by `ctx` alone).
    pub plugin: u32,
    /// Calls a server method with JSON arguments; answers JSON, freed with
    /// `free`.
    pub call:
        unsafe extern "C" fn(ctx: *mut c_void, plugin: u32, method: RawStr, args: RawStr) -> RawBuf,
    pub free: unsafe extern "C" fn(buf: RawBuf),
}

/// What a plugin gives the server.
#[repr(C)]
pub struct PluginApi {
    pub abi: u32,
    pub instance: *mut c_void,
    /// Hands the plugin a message (`describe`, `load`, `enable`, `disable`,
    /// `event`, `command`, `tab`, `tick`); answers JSON, freed with `free`.
    pub handle:
        unsafe extern "C" fn(instance: *mut c_void, kind: RawStr, payload: RawStr) -> RawBuf,
    pub free: unsafe extern "C" fn(buf: RawBuf),
    /// Drops the plugin; no call follows.
    pub destroy: unsafe extern "C" fn(instance: *mut c_void),
}

/// The type of [`ENTRY_SYMBOL`].
pub type EntryFn = unsafe extern "C" fn(host: *const HostApi) -> PluginApi;
