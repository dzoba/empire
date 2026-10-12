//! WebAssembly entry points for the browser host (`web/main.js`).
//!
//! A plain C ABI: strings and byte buffers are passed as (pointer, length)
//! pairs in linear memory, allocated with `empire_alloc`. There is no
//! wasm-bindgen, so building needs only `cargo` and the
//! `wasm32-unknown-unknown` target.

use empire_app::{App, KeyEvent};
use std::cell::RefCell;

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

// Imported from the JavaScript host (web/main.js, `imports.env`).
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    fn host_log(ptr: *const u8, len: usize);
}

fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        host_log(msg.as_ptr(), msg.len());
    }
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{msg}");
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| f(&mut a.borrow_mut()))
}

/// # Safety
/// `ptr` must point to `len` readable bytes in this module's memory.
unsafe fn read_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
}

/// # Safety
/// `ptr` must point to `len` readable bytes in this module's memory.
unsafe fn read_bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    std::slice::from_raw_parts(ptr, len)
}

#[no_mangle]
pub extern "C" fn empire_alloc(len: usize) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` must come from `empire_alloc`.
#[no_mangle]
pub unsafe extern "C" fn empire_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

#[no_mangle]
pub extern "C" fn empire_init() {
    std::panic::set_hook(Box::new(|info| log(&format!("panic: {info}"))));
    with_app(|_| ());
}

/// Window size in CSS pixels and `window.devicePixelRatio`.
#[no_mangle]
pub extern "C" fn empire_set_viewport(w: u32, h: u32, dpr: f64) {
    with_app(|a| a.set_viewport(w, h, dpr));
}

/// Logical (layout) size: mouse coordinates are given in these units.
#[no_mangle]
pub extern "C" fn empire_width() -> u32 {
    with_app(|a| a.width() as u32)
}

#[no_mangle]
pub extern "C" fn empire_height() -> u32 {
    with_app(|a| a.height() as u32)
}

/// Canvas buffer size in device pixels.
#[no_mangle]
pub extern "C" fn empire_pixel_width() -> u32 {
    with_app(|a| a.pixel_width() as u32)
}

#[no_mangle]
pub extern "C" fn empire_pixel_height() -> u32 {
    with_app(|a| a.pixel_height() as u32)
}

/// Size to show the canvas at, in CSS pixels.
#[no_mangle]
pub extern "C" fn empire_display_width() -> f64 {
    with_app(|a| a.display_size().0)
}

#[no_mangle]
pub extern "C" fn empire_display_height() -> f64 {
    with_app(|a| a.display_size().1)
}

/// Advance time; returns 1 if the screen needs to be redrawn.
#[no_mangle]
pub extern "C" fn empire_tick(now_ms: f64) -> u32 {
    with_app(|a| a.tick(now_ms) as u32)
}

/// Draw and return a pointer to `pixel_width * pixel_height * 4` RGBA bytes.
#[no_mangle]
pub extern "C" fn empire_render() -> *const u8 {
    with_app(|a| a.render().as_ptr())
}

/// Keyboard: DOM `key` and `code`, modifiers as bits (1 ctrl, 2 alt, 4 shift, 8 meta).
/// Returns 1 if the game used the key (the host should `preventDefault`).
///
/// # Safety
/// Pointers must reference valid UTF-8 of the given lengths.
#[no_mangle]
pub unsafe extern "C" fn empire_key(key: *const u8, key_len: usize, code: *const u8, code_len: usize, mods: u32) -> u32 {
    let key = read_str(key, key_len);
    let code = read_str(code, code_len);
    if mods & 8 != 0 {
        return 0; // leave Cmd/Win shortcuts to the browser
    }
    match KeyEvent::from_web(key, code, mods & 1 != 0, mods & 2 != 0, mods & 4 != 0) {
        Some(ev) => with_app(|a| a.key_down(ev) as u32),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn empire_mouse_down(x: i32, y: i32, button: u32) {
    with_app(|a| a.mouse_down(x, y, button as u8));
}

#[no_mangle]
pub extern "C" fn empire_mouse_move(x: i32, y: i32) {
    with_app(|a| a.mouse_move(x, y));
}

#[no_mangle]
pub extern "C" fn empire_wheel(dx: i32, dy: i32) {
    with_app(|a| a.wheel(dx, dy));
}

/// # Safety
/// Pointers must reference valid UTF-8 of the given lengths.
#[no_mangle]
pub unsafe extern "C" fn empire_storage(key: *const u8, key_len: usize, val: *const u8, val_len: usize) {
    let (k, v) = (read_str(key, key_len), read_str(val, val_len));
    with_app(|a| a.storage_value(k, v));
}

/// # Safety
/// Pointers must reference valid UTF-8 of the given lengths.
#[no_mangle]
pub unsafe extern "C" fn empire_asset_text(name: *const u8, name_len: usize, text: *const u8, text_len: usize) -> u32 {
    let (n, t) = (read_str(name, name_len), read_str(text, text_len));
    with_app(|a| match a.load_asset_text(n, t) {
        Ok(()) => 1,
        Err(e) => {
            log(&format!("{n}: {e}"));
            0
        }
    })
}

/// # Safety
/// `name` must be valid UTF-8; `data` must reference `data_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn empire_asset_bytes(name: *const u8, name_len: usize, data: *const u8, data_len: usize) -> u32 {
    let n = read_str(name, name_len);
    let d = read_bytes(data, data_len);
    with_app(|a| a.load_asset_bytes(n, d).is_ok() as u32)
}

/// # Safety
/// Pointers must reference valid UTF-8 of the given lengths.
#[no_mangle]
pub unsafe extern "C" fn empire_file_loaded(name: *const u8, name_len: usize, text: *const u8, text_len: usize) {
    let (n, t) = (read_str(name, name_len), read_str(text, text_len));
    with_app(|a| a.file_loaded(n, t));
}

/// Pending host requests as a JSON array; read `empire_requests_len` bytes.
#[no_mangle]
pub extern "C" fn empire_requests() -> *const u8 {
    let json = with_app(|a| a.take_requests_json());
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = json.into_bytes();
        o.as_ptr()
    })
}

#[no_mangle]
pub extern "C" fn empire_requests_len() -> usize {
    OUT.with(|o| o.borrow().len())
}
