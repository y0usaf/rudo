//! Minimal FreeType FFI via dlopen — zero compile-time dependency.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use crate::dlopen::{DlLibrary, Symbol};
use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ushort};
use std::sync::OnceLock;

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
}

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
    pub format: u32,
    pub bitmap: FT_Bitmap,
    pub bitmap_left: c_int,
    pub bitmap_top: c_int,
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

pub const FT_LOAD_DEFAULT: FT_Int32 = 0;
pub const FT_LOAD_RENDER: FT_Int32 = 1 << 2;

pub struct FtHandle {
    _lib: *mut c_void,
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
    pub set_char_size:
        unsafe extern "C" fn(FT_Face, FT_F26Dot6, FT_F26Dot6, FT_UInt, FT_UInt) -> FT_Error,
    pub load_char: unsafe extern "C" fn(FT_Face, FT_ULong, FT_Int32) -> FT_Error,
    pub get_char_index: unsafe extern "C" fn(FT_Face, FT_ULong) -> FT_UInt,
}

unsafe impl Send for FtHandle {}
unsafe impl Sync for FtHandle {}

impl FtHandle {
    fn load() -> Option<Self> {
        const LIBFREETYPE_SO_6: &[u8] = b"libfreetype.so.6\0";
        const LIBFREETYPE_SO: &[u8] = b"libfreetype.so\0";

        const FT_INIT_FREETYPE: Symbol<unsafe extern "C" fn(*mut FT_Library) -> FT_Error> =
            Symbol::new(b"FT_Init_FreeType\0");
        const FT_DONE_FREETYPE: Symbol<unsafe extern "C" fn(FT_Library) -> FT_Error> =
            Symbol::new(b"FT_Done_FreeType\0");
        const FT_NEW_MEMORY_FACE: Symbol<
            unsafe extern "C" fn(
                FT_Library,
                *const c_uchar,
                FT_Long,
                FT_Long,
                *mut FT_Face,
            ) -> FT_Error,
        > = Symbol::new(b"FT_New_Memory_Face\0");
        const FT_DONE_FACE: Symbol<unsafe extern "C" fn(FT_Face) -> FT_Error> =
            Symbol::new(b"FT_Done_Face\0");
        const FT_SET_PIXEL_SIZES: Symbol<
            unsafe extern "C" fn(FT_Face, FT_UInt, FT_UInt) -> FT_Error,
        > = Symbol::new(b"FT_Set_Pixel_Sizes\0");
        const FT_SET_CHAR_SIZE: Symbol<
            unsafe extern "C" fn(FT_Face, FT_F26Dot6, FT_F26Dot6, FT_UInt, FT_UInt) -> FT_Error,
        > = Symbol::new(b"FT_Set_Char_Size\0");
        const FT_LOAD_CHAR: Symbol<unsafe extern "C" fn(FT_Face, FT_ULong, FT_Int32) -> FT_Error> =
            Symbol::new(b"FT_Load_Char\0");
        const FT_GET_CHAR_INDEX: Symbol<unsafe extern "C" fn(FT_Face, FT_ULong) -> FT_UInt> =
            Symbol::new(b"FT_Get_Char_Index\0");

        unsafe {
            let library = DlLibrary::open_any(&[LIBFREETYPE_SO_6, LIBFREETYPE_SO])?;
            let init_freetype = FT_INIT_FREETYPE.get(&library)?;
            let done_freetype = FT_DONE_FREETYPE.get(&library)?;
            let new_memory_face = FT_NEW_MEMORY_FACE.get(&library)?;
            let done_face = FT_DONE_FACE.get(&library)?;
            let set_pixel_sizes = FT_SET_PIXEL_SIZES.get(&library)?;
            let set_char_size = FT_SET_CHAR_SIZE.get(&library)?;
            let load_char = FT_LOAD_CHAR.get(&library)?;
            let get_char_index = FT_GET_CHAR_INDEX.get(&library)?;
            let _lib = library.into_raw();

            Some(Self {
                _lib,
                init_freetype,
                done_freetype,
                new_memory_face,
                done_face,
                set_pixel_sizes,
                set_char_size,
                load_char,
                get_char_index,
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
