//! Minimal FreeType FFI via dlopen — zero compile-time dependency.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ushort};
use std::sync::OnceLock;

// ── FreeType types ───────────────────────────────────────────────────────────

pub type FT_Error = c_int;
pub type FT_Library = *mut c_void;
pub type FT_Face = *mut FT_FaceRec;
pub type FT_Int32 = i32;
pub type FT_UInt = c_uint;
pub type FT_ULong = c_long;
pub type FT_Long = c_long;
pub type FT_F26Dot6 = c_long;
pub type FT_Pos = c_long;
pub type FT_Fixed = c_long;

#[repr(C)]
pub struct FT_FaceRec {
    pub num_faces: FT_Long,
    pub face_index: FT_Long,
    pub face_flags: FT_Long,
    pub style_flags: FT_Long,
    pub num_glyphs: FT_Long,
    pub family_name: *mut c_char,
    pub style_name: *mut c_char,
    pub num_fixed_sizes: c_int,
    pub available_sizes: *mut c_void,
    pub num_charmaps: c_int,
    pub charmaps: *mut c_void,
    pub generic: FT_Generic,
    pub bbox: FT_BBox,
    pub units_per_em: c_ushort,
    pub ascender: c_short,
    pub descender: c_short,
    pub height: c_short,
    pub max_advance_width: c_short,
    pub max_advance_height: c_short,
    pub underline_position: c_short,
    pub underline_thickness: c_short,
    pub glyph: *mut FT_GlyphSlotRec,
    pub size: *mut FT_SizeRec,
    // ... more fields follow but we don't need them
}

use std::os::raw::c_short;

#[repr(C)]
pub struct FT_Generic {
    pub data: *mut c_void,
    pub finalizer: *mut c_void,
}

#[repr(C)]
pub struct FT_BBox {
    pub xmin: FT_Pos,
    pub ymin: FT_Pos,
    pub xmax: FT_Pos,
    pub ymax: FT_Pos,
}

#[repr(C)]
pub struct FT_GlyphSlotRec {
    pub library: FT_Library,
    pub face: FT_Face,
    pub next: *mut FT_GlyphSlotRec,
    pub glyph_index: FT_UInt,
    pub generic: FT_Generic,
    pub metrics: FT_Glyph_Metrics,
    pub linearHoriAdvance: FT_Fixed,
    pub linearVertAdvance: FT_Fixed,
    pub advance: FT_Vector,
    pub format: u32, // FT_Glyph_Format
    pub bitmap: FT_Bitmap,
    pub bitmap_left: c_int,
    pub bitmap_top: c_int,
    // ... more fields follow
}

#[repr(C)]
pub struct FT_Glyph_Metrics {
    pub width: FT_Pos,
    pub height: FT_Pos,
    pub horiBearingX: FT_Pos,
    pub horiBearingY: FT_Pos,
    pub horiAdvance: FT_Pos,
    pub vertBearingX: FT_Pos,
    pub vertBearingY: FT_Pos,
    pub vertAdvance: FT_Pos,
}

#[repr(C)]
pub struct FT_Bitmap {
    pub rows: c_uint,
    pub width: c_uint,
    pub pitch: c_int,
    pub buffer: *mut c_uchar,
    pub num_grays: c_ushort,
    pub pixel_mode: c_uchar,
    pub palette_mode: c_uchar,
    pub palette: *mut c_void,
}

#[repr(C)]
pub struct FT_Vector {
    pub x: FT_Pos,
    pub y: FT_Pos,
}

#[repr(C)]
pub struct FT_SizeRec {
    pub face: FT_Face,
    pub generic: FT_Generic,
    pub metrics: FT_Size_Metrics,
    // ...
}

#[repr(C)]
pub struct FT_Size_Metrics {
    pub x_ppem: c_ushort,
    pub y_ppem: c_ushort,
    pub x_scale: FT_Fixed,
    pub y_scale: FT_Fixed,
    pub ascender: FT_Pos,
    pub descender: FT_Pos,
    pub height: FT_Pos,
    pub max_advance: FT_Pos,
}

// ── Load flags ───────────────────────────────────────────────────────────────

pub const FT_LOAD_RENDER: FT_Int32 = 1 << 2;
pub const FT_LOAD_NO_HINTING: FT_Int32 = 1 << 1;

// ── Function table ───────────────────────────────────────────────────────────

pub struct FtHandle {
    _lib: *mut c_void, // dlopen handle
    pub init_freetype: unsafe extern "C" fn(*mut FT_Library) -> FT_Error,
    pub done_freetype: unsafe extern "C" fn(FT_Library) -> FT_Error,
    pub new_memory_face: unsafe extern "C" fn(
        FT_Library,
        *const c_uchar,
        FT_Long,
        FT_Long,
        *mut FT_Face,
    ) -> FT_Error,
    pub done_face: unsafe extern "C" fn(FT_Face) -> FT_Error,
    pub set_pixel_sizes: unsafe extern "C" fn(FT_Face, FT_UInt, FT_UInt) -> FT_Error,
    pub load_char: unsafe extern "C" fn(FT_Face, FT_ULong, FT_Int32) -> FT_Error,
    pub get_char_index: unsafe extern "C" fn(FT_Face, FT_ULong) -> FT_UInt,
}

unsafe impl Send for FtHandle {}
unsafe impl Sync for FtHandle {}

impl FtHandle {
    fn load() -> Option<Self> {
        unsafe {
            let names: &[&[u8]] = &[b"libfreetype.so.6\0", b"libfreetype.so\0"];
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
                init_freetype: sym!("FT_Init_FreeType"),
                done_freetype: sym!("FT_Done_FreeType"),
                new_memory_face: sym!("FT_New_Memory_Face"),
                done_face: sym!("FT_Done_Face"),
                set_pixel_sizes: sym!("FT_Set_Pixel_Sizes"),
                load_char: sym!("FT_Load_Char"),
                get_char_index: sym!("FT_Get_Char_Index"),
            })
        }
    }
}

static FT_HANDLE: OnceLock<FtHandle> = OnceLock::new();

pub fn ft() -> &'static FtHandle {
    FT_HANDLE.get_or_init(|| {
        FtHandle::load().expect("Failed to load libfreetype.so — is freetype2 installed?")
    })
}
