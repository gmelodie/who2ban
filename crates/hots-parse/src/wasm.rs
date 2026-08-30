//! Raw ABI for the browser, so the page needs no wasm-bindgen and no build step.
//!
//! The page calls `hots_alloc`, writes the file bytes, calls a parser and reads the
//! answer at `[u32 length][json]`, then frees both buffers.

use std::alloc::{Layout, alloc, dealloc};

#[unsafe(no_mangle)]
pub extern "C" fn hots_alloc(len: usize) -> *mut u8 {
    match layout(len) {
        Some(layout) => unsafe { alloc(layout) },
        None => std::ptr::null_mut(),
    }
}

/// # Safety: `ptr` and `len` must come from [`hots_alloc`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hots_free(ptr: *mut u8, len: usize) {
    if let Some(layout) = layout(len)
        && !ptr.is_null()
    {
        unsafe { dealloc(ptr, layout) };
    }
}

/// # Safety: `ptr` must point at `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hots_parse_lobby(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    answer(crate::parse::battlelobby(bytes))
}

/// # Safety: `ptr` must point at `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hots_parse_replay(ptr: *const u8, len: usize) -> *mut u8 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    answer(crate::parse::replay_bytes(bytes))
}

fn layout(len: usize) -> Option<Layout> {
    (len > 0).then(|| Layout::array::<u8>(len).ok())?
}

fn answer<T: serde::Serialize>(result: crate::Result<T>) -> *mut u8 {
    let json = match result {
        Ok(value) => {
            serde_json::to_string(&value).unwrap_or_else(|e| error_json(&format!("serialize: {e}")))
        }
        Err(e) => error_json(&e.to_string()),
    };
    write_out(&json)
}

fn error_json(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

fn write_out(json: &str) -> *mut u8 {
    let bytes = json.as_bytes();
    let ptr = hots_alloc(4 + bytes.len());
    if ptr.is_null() {
        return ptr;
    }
    unsafe {
        std::ptr::copy_nonoverlapping((bytes.len() as u32).to_le_bytes().as_ptr(), ptr, 4);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.add(4), bytes.len());
    }
    ptr
}
