//! Minimal fontconfig FFI via dlopen — zero compile-time dependency.
#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]

use crate::dlopen::{DlLibrary, Symbol};
use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uchar};
use std::sync::OnceLock;

pub type FcChar8 = c_uchar;
pub type FcBool = c_int;
pub type FcResult = c_int;
pub type FcMatchKind = c_int;

pub type FcConfig = c_void;
pub type FcPattern = c_void;

#[repr(C)]
pub struct FcFontSet {
    pub nfont: c_int,
    pub sfont: c_int,
    pub fonts: *mut *mut FcPattern,
}

pub const FcTrue: FcBool = 1;
pub const FcMatchPattern: FcMatchKind = 0;
pub const FcResultMatch: FcResult = 0;
pub const FcResultNoMatch: FcResult = 1;
pub const FC_FILE: &[u8] = b"file\0";

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
        const LIBFONTCONFIG_SO_1: &[u8] = b"libfontconfig.so.1\0";
        const LIBFONTCONFIG_SO: &[u8] = b"libfontconfig.so\0";

        const FC_INIT_LOAD_CONFIG_AND_FONTS: Symbol<unsafe extern "C" fn() -> *mut FcConfig> =
            Symbol::new(b"FcInitLoadConfigAndFonts\0");
        const FC_CONFIG_DESTROY: Symbol<unsafe extern "C" fn(*mut FcConfig)> =
            Symbol::new(b"FcConfigDestroy\0");
        const FC_NAME_PARSE: Symbol<unsafe extern "C" fn(*const FcChar8) -> *mut FcPattern> =
            Symbol::new(b"FcNameParse\0");
        const FC_PATTERN_DESTROY: Symbol<unsafe extern "C" fn(*mut FcPattern)> =
            Symbol::new(b"FcPatternDestroy\0");
        const FC_CONFIG_SUBSTITUTE: Symbol<
            unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, FcMatchKind) -> FcBool,
        > = Symbol::new(b"FcConfigSubstitute\0");
        const FC_DEFAULT_SUBSTITUTE: Symbol<unsafe extern "C" fn(*mut FcPattern)> =
            Symbol::new(b"FcDefaultSubstitute\0");
        const FC_FONT_MATCH: Symbol<
            unsafe extern "C" fn(*mut FcConfig, *mut FcPattern, *mut FcResult) -> *mut FcPattern,
        > = Symbol::new(b"FcFontMatch\0");
        const FC_FONT_SORT: Symbol<
            unsafe extern "C" fn(
                *mut FcConfig,
                *mut FcPattern,
                FcBool,
                *mut *mut c_void,
                *mut FcResult,
            ) -> *mut FcFontSet,
        > = Symbol::new(b"FcFontSort\0");
        const FC_FONT_SET_DESTROY: Symbol<unsafe extern "C" fn(*mut FcFontSet)> =
            Symbol::new(b"FcFontSetDestroy\0");
        const FC_PATTERN_GET_STRING: Symbol<
            unsafe extern "C" fn(
                *const FcPattern,
                *const c_char,
                c_int,
                *mut *mut FcChar8,
            ) -> FcResult,
        > = Symbol::new(b"FcPatternGetString\0");

        unsafe {
            let library = DlLibrary::open_any(&[LIBFONTCONFIG_SO_1, LIBFONTCONFIG_SO])?;
            let FcInitLoadConfigAndFonts = FC_INIT_LOAD_CONFIG_AND_FONTS.get(&library)?;
            let FcConfigDestroy = FC_CONFIG_DESTROY.get(&library)?;
            let FcNameParse = FC_NAME_PARSE.get(&library)?;
            let FcPatternDestroy = FC_PATTERN_DESTROY.get(&library)?;
            let FcConfigSubstitute = FC_CONFIG_SUBSTITUTE.get(&library)?;
            let FcDefaultSubstitute = FC_DEFAULT_SUBSTITUTE.get(&library)?;
            let FcFontMatch = FC_FONT_MATCH.get(&library)?;
            let FcFontSort = FC_FONT_SORT.get(&library)?;
            let FcFontSetDestroy = FC_FONT_SET_DESTROY.get(&library)?;
            let FcPatternGetString = FC_PATTERN_GET_STRING.get(&library)?;
            let _lib = library.into_raw();

            Some(Self {
                _lib,
                FcInitLoadConfigAndFonts,
                FcConfigDestroy,
                FcNameParse,
                FcPatternDestroy,
                FcConfigSubstitute,
                FcDefaultSubstitute,
                FcFontMatch,
                FcFontSort,
                FcFontSetDestroy,
                FcPatternGetString,
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
