//! Minimal fontconfig FFI via dlopen — zero compile-time dependency.
#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uchar};
use std::sync::OnceLock;

// ── Opaque/public C types ───────────────────────────────────────────────────

pub type FcChar8 = c_uchar;
pub type FcBool = c_int;
pub type FcResult = c_int;
pub type FcMatchKind = c_int;

pub type FcConfig = c_void;
pub type FcPattern = c_void;

// FcFontSet is not opaque because callers may need to iterate sorted fallback
// candidates returned by FcFontSort.
#[repr(C)]
pub struct FcFontSet {
    pub nfont: c_int,
    pub sfont: c_int,
    pub fonts: *mut *mut FcPattern,
}

// ── Constants ────────────────────────────────────────────────────────────────

pub const FcFalse: FcBool = 0;
pub const FcTrue: FcBool = 1;

pub const FcMatchPattern: FcMatchKind = 0;
pub const FcMatchFont: FcMatchKind = 1;
pub const FcMatchScan: FcMatchKind = 2;

pub const FcResultMatch: FcResult = 0;
pub const FcResultNoMatch: FcResult = 1;
pub const FcResultTypeMismatch: FcResult = 2;
pub const FcResultNoId: FcResult = 3;
pub const FcResultOutOfMemory: FcResult = 4;

pub const FC_FAMILY: &[u8] = b"family\0";
pub const FC_STYLE: &[u8] = b"style\0";
pub const FC_FILE: &[u8] = b"file\0";

// ── Function table ───────────────────────────────────────────────────────────

pub struct FontconfigHandle {
    _lib: *mut c_void,
    pub FcInitLoadConfigAndFonts: unsafe extern "C" fn() -> *mut FcConfig,
    pub FcConfigDestroy: unsafe extern "C" fn(*mut FcConfig),
    pub FcNameParse: unsafe extern "C" fn(*const FcChar8) -> *mut FcPattern,
    pub FcPatternDestroy: unsafe extern "C" fn(*mut FcPattern),
    pub FcConfigSubstitute:
        unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, FcMatchKind) -> FcBool,
    pub FcDefaultSubstitute: unsafe extern "C" fn(*mut FcPattern),
    pub FcFontMatch:
        unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, *mut FcResult) -> *mut FcPattern,
    pub FcFontSort: unsafe extern "C" fn(
        *mut FcConfig,
        *mut FcPattern,
        FcBool,
        *mut *mut c_void,
        *mut FcResult,
    ) -> *mut FcFontSet,
    pub FcFontSetDestroy: unsafe extern "C" fn(*mut FcFontSet),
    pub FcPatternGetString:
        unsafe extern "C" fn(*const FcPattern, *const c_char, c_int, *mut *mut FcChar8) -> FcResult,
}

unsafe impl Send for FontconfigHandle {}
unsafe impl Sync for FontconfigHandle {}

impl FontconfigHandle {
    fn load() -> Option<Self> {
        unsafe {
            let names: &[&[u8]] = &[b"libfontconfig.so.1\0", b"libfontconfig.so\0"];
            let mut handle = std::ptr::null_mut();
            for name in names {
                handle = libc::dlopen(name.as_ptr().cast(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
                if !handle.is_null() {
                    break;
                }
            }
            if handle.is_null() {
                return None;
            }

            macro_rules! sym {
                ($name:literal) => {{
                    let p = libc::dlsym(handle, concat!($name, "\0").as_ptr().cast());
                    if p.is_null() {
                        libc::dlclose(handle);
                        return None;
                    }
                    std::mem::transmute(p)
                }};
            }

            Some(Self {
                _lib: handle,
                FcInitLoadConfigAndFonts: sym!("FcInitLoadConfigAndFonts"),
                FcConfigDestroy: sym!("FcConfigDestroy"),
                FcNameParse: sym!("FcNameParse"),
                FcPatternDestroy: sym!("FcPatternDestroy"),
                FcConfigSubstitute: sym!("FcConfigSubstitute"),
                FcDefaultSubstitute: sym!("FcDefaultSubstitute"),
                FcFontMatch: sym!("FcFontMatch"),
                FcFontSort: sym!("FcFontSort"),
                FcFontSetDestroy: sym!("FcFontSetDestroy"),
                FcPatternGetString: sym!("FcPatternGetString"),
            })
        }
    }
}

static FONTCONFIG_HANDLE: OnceLock<FontconfigHandle> = OnceLock::new();

pub fn try_fontconfig() -> Option<&'static FontconfigHandle> {
    if let Some(handle) = FONTCONFIG_HANDLE.get() {
        return Some(handle);
    }
    let handle = FontconfigHandle::load()?;
    let _ = FONTCONFIG_HANDLE.set(handle);
    FONTCONFIG_HANDLE.get()
}

pub fn fontconfig() -> &'static FontconfigHandle {
    try_fontconfig().expect("Failed to load libfontconfig.so — is fontconfig installed?")
}
