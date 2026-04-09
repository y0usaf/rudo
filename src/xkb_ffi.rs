//! Minimal xkbcommon FFI via dlopen — zero compile-time dependency.
#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use crate::dlopen::{DlLibrary, Symbol};
use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};
use std::sync::OnceLock;

pub type xkb_context = c_void;
pub type xkb_keymap = c_void;
pub type xkb_state = c_void;
pub type xkb_keycode_t = u32;
pub type xkb_keysym_t = u32;
pub type xkb_mod_mask_t = u32;
pub type xkb_layout_index_t = u32;

pub const XKB_CONTEXT_NO_FLAGS: c_int = 0;
pub const XKB_KEYMAP_FORMAT_TEXT_V1: c_int = 1;
pub const XKB_KEYMAP_COMPILE_NO_FLAGS: c_int = 0;
pub const XKB_KEY_DOWN: c_int = 1;
pub const XKB_KEY_UP: c_int = 0;
pub const XKB_STATE_MODS_EFFECTIVE: c_uint = 1 << 3;

pub const XKB_MOD_NAME_SHIFT: &[u8] = b"Shift\0";
pub const XKB_MOD_NAME_CTRL: &[u8] = b"Control\0";
pub const XKB_MOD_NAME_ALT: &[u8] = b"Mod1\0";

pub const XKB_KEY_Return: xkb_keysym_t = 0xff0d;
pub const XKB_KEY_BackSpace: xkb_keysym_t = 0xff08;
pub const XKB_KEY_Escape: xkb_keysym_t = 0xff1b;
pub const XKB_KEY_Tab: xkb_keysym_t = 0xff09;
pub const XKB_KEY_Left: xkb_keysym_t = 0xff51;
pub const XKB_KEY_Right: xkb_keysym_t = 0xff53;
pub const XKB_KEY_Up: xkb_keysym_t = 0xff52;
pub const XKB_KEY_Down: xkb_keysym_t = 0xff54;
pub const XKB_KEY_Home: xkb_keysym_t = 0xff50;
pub const XKB_KEY_End: xkb_keysym_t = 0xff57;
pub const XKB_KEY_Prior: xkb_keysym_t = 0xff55;
pub const XKB_KEY_Next: xkb_keysym_t = 0xff56;
pub const XKB_KEY_Delete: xkb_keysym_t = 0xffff;
pub const XKB_KEY_Insert: xkb_keysym_t = 0xff63;
pub const XKB_KEY_F1: xkb_keysym_t = 0xffbe;
pub const XKB_KEY_F2: xkb_keysym_t = 0xffbf;
pub const XKB_KEY_F3: xkb_keysym_t = 0xffc0;
pub const XKB_KEY_F4: xkb_keysym_t = 0xffc1;
pub const XKB_KEY_F5: xkb_keysym_t = 0xffc2;
pub const XKB_KEY_F6: xkb_keysym_t = 0xffc3;
pub const XKB_KEY_F7: xkb_keysym_t = 0xffc4;
pub const XKB_KEY_F8: xkb_keysym_t = 0xffc5;
pub const XKB_KEY_F9: xkb_keysym_t = 0xffc6;
pub const XKB_KEY_F10: xkb_keysym_t = 0xffc7;
pub const XKB_KEY_F11: xkb_keysym_t = 0xffc8;
pub const XKB_KEY_F12: xkb_keysym_t = 0xffc9;

pub struct XkbHandle {
    _lib: *mut c_void,
    pub xkb_context_new: unsafe extern "C" fn(c_int) -> *mut xkb_context,
    pub xkb_context_unref: unsafe extern "C" fn(*mut xkb_context),
    pub xkb_keymap_new_from_buffer: unsafe extern "C" fn(
        *mut xkb_context,
        *const c_char,
        usize,
        c_int,
        c_int,
    ) -> *mut xkb_keymap,
    pub xkb_keymap_unref: unsafe extern "C" fn(*mut xkb_keymap),
    pub xkb_keymap_key_repeats: unsafe extern "C" fn(*mut xkb_keymap, xkb_keycode_t) -> c_int,
    pub xkb_state_new: unsafe extern "C" fn(*mut xkb_keymap) -> *mut xkb_state,
    pub xkb_state_unref: unsafe extern "C" fn(*mut xkb_state),
    pub xkb_state_update_key: unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t, c_int) -> c_uint,
    pub xkb_state_update_mask: unsafe extern "C" fn(
        *mut xkb_state,
        xkb_mod_mask_t,
        xkb_mod_mask_t,
        xkb_mod_mask_t,
        xkb_layout_index_t,
        xkb_layout_index_t,
        xkb_layout_index_t,
    ) -> c_uint,
    pub xkb_state_key_get_one_sym:
        unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t) -> xkb_keysym_t,
    pub xkb_state_key_get_utf8:
        unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t, *mut c_char, usize) -> c_int,
    pub xkb_state_mod_name_is_active:
        unsafe extern "C" fn(*mut xkb_state, *const c_char, c_uint) -> c_int,
}

unsafe impl Send for XkbHandle {}
unsafe impl Sync for XkbHandle {}

impl XkbHandle {
    fn load() -> Option<Self> {
        const LIBXKBCOMMON_SO_0: &[u8] = b"libxkbcommon.so.0\0";
        const LIBXKBCOMMON_SO: &[u8] = b"libxkbcommon.so\0";

        const XKB_CONTEXT_NEW: Symbol<unsafe extern "C" fn(c_int) -> *mut xkb_context> =
            Symbol::new(b"xkb_context_new\0");
        const XKB_CONTEXT_UNREF: Symbol<unsafe extern "C" fn(*mut xkb_context)> =
            Symbol::new(b"xkb_context_unref\0");
        const XKB_KEYMAP_NEW_FROM_BUFFER: Symbol<
            unsafe extern "C" fn(
                *mut xkb_context,
                *const c_char,
                usize,
                c_int,
                c_int,
            ) -> *mut xkb_keymap,
        > = Symbol::new(b"xkb_keymap_new_from_buffer\0");
        const XKB_KEYMAP_UNREF: Symbol<unsafe extern "C" fn(*mut xkb_keymap)> =
            Symbol::new(b"xkb_keymap_unref\0");
        const XKB_KEYMAP_KEY_REPEATS: Symbol<
            unsafe extern "C" fn(*mut xkb_keymap, xkb_keycode_t) -> c_int,
        > = Symbol::new(b"xkb_keymap_key_repeats\0");
        const XKB_STATE_NEW: Symbol<unsafe extern "C" fn(*mut xkb_keymap) -> *mut xkb_state> =
            Symbol::new(b"xkb_state_new\0");
        const XKB_STATE_UNREF: Symbol<unsafe extern "C" fn(*mut xkb_state)> =
            Symbol::new(b"xkb_state_unref\0");
        const XKB_STATE_UPDATE_KEY: Symbol<
            unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t, c_int) -> c_uint,
        > = Symbol::new(b"xkb_state_update_key\0");
        const XKB_STATE_UPDATE_MASK: Symbol<
            unsafe extern "C" fn(
                *mut xkb_state,
                xkb_mod_mask_t,
                xkb_mod_mask_t,
                xkb_mod_mask_t,
                xkb_layout_index_t,
                xkb_layout_index_t,
                xkb_layout_index_t,
            ) -> c_uint,
        > = Symbol::new(b"xkb_state_update_mask\0");
        const XKB_STATE_KEY_GET_ONE_SYM: Symbol<
            unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t) -> xkb_keysym_t,
        > = Symbol::new(b"xkb_state_key_get_one_sym\0");
        const XKB_STATE_KEY_GET_UTF8: Symbol<
            unsafe extern "C" fn(*mut xkb_state, xkb_keycode_t, *mut c_char, usize) -> c_int,
        > = Symbol::new(b"xkb_state_key_get_utf8\0");
        const XKB_STATE_MOD_NAME_IS_ACTIVE: Symbol<
            unsafe extern "C" fn(*mut xkb_state, *const c_char, c_uint) -> c_int,
        > = Symbol::new(b"xkb_state_mod_name_is_active\0");

        unsafe {
            let library = DlLibrary::open_any(&[LIBXKBCOMMON_SO_0, LIBXKBCOMMON_SO])?;
            let xkb_context_new = XKB_CONTEXT_NEW.get(&library)?;
            let xkb_context_unref = XKB_CONTEXT_UNREF.get(&library)?;
            let xkb_keymap_new_from_buffer = XKB_KEYMAP_NEW_FROM_BUFFER.get(&library)?;
            let xkb_keymap_unref = XKB_KEYMAP_UNREF.get(&library)?;
            let xkb_keymap_key_repeats = XKB_KEYMAP_KEY_REPEATS.get(&library)?;
            let xkb_state_new = XKB_STATE_NEW.get(&library)?;
            let xkb_state_unref = XKB_STATE_UNREF.get(&library)?;
            let xkb_state_update_key = XKB_STATE_UPDATE_KEY.get(&library)?;
            let xkb_state_update_mask = XKB_STATE_UPDATE_MASK.get(&library)?;
            let xkb_state_key_get_one_sym = XKB_STATE_KEY_GET_ONE_SYM.get(&library)?;
            let xkb_state_key_get_utf8 = XKB_STATE_KEY_GET_UTF8.get(&library)?;
            let xkb_state_mod_name_is_active = XKB_STATE_MOD_NAME_IS_ACTIVE.get(&library)?;
            let _lib = library.into_raw();

            Some(Self {
                _lib,
                xkb_context_new,
                xkb_context_unref,
                xkb_keymap_new_from_buffer,
                xkb_keymap_unref,
                xkb_keymap_key_repeats,
                xkb_state_new,
                xkb_state_unref,
                xkb_state_update_key,
                xkb_state_update_mask,
                xkb_state_key_get_one_sym,
                xkb_state_key_get_utf8,
                xkb_state_mod_name_is_active,
            })
        }
    }
}

static XKB_HANDLE: OnceLock<XkbHandle> = OnceLock::new();

pub fn xkb() -> &'static XkbHandle {
    XKB_HANDLE.get_or_init(|| {
        XkbHandle::load().expect("Failed to load libxkbcommon.so — is xkbcommon installed?")
    })
}
