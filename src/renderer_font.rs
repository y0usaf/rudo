use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::ptr::NonNull;
use std::slice;

use crate::contracts::CheckInvariant;

const ATLAS_WIDTH: u32 = 1024;
const ATLAS_HEIGHT: u32 = 1024;
const DEFAULT_FONT_FAMILY: &str = "monospace";

#[repr(C)]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: f32,
    pub height: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

#[repr(C)]
struct CFontAtlas {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn rudo_font_atlas_new(font_size: f32, preferred_family: *const c_char) -> *mut CFontAtlas;
    fn rudo_font_atlas_free(ptr: *mut CFontAtlas);
    fn rudo_font_atlas_cell_width(ptr: *const CFontAtlas) -> f32;
    fn rudo_font_atlas_cell_height(ptr: *const CFontAtlas) -> f32;
    fn rudo_font_atlas_baseline(ptr: *const CFontAtlas) -> f32;
    fn rudo_font_atlas_get_glyph(
        ptr: *mut CFontAtlas,
        ch: u32,
        bold: c_int,
        italic: c_int,
    ) -> GlyphInfo;
    fn rudo_font_atlas_data(ptr: *const CFontAtlas, len_out: *mut usize) -> *const u8;
    fn rudo_font_atlas_width(ptr: *const CFontAtlas) -> u32;
    fn rudo_font_atlas_height(ptr: *const CFontAtlas) -> u32;
}

pub struct FontAtlas {
    raw: NonNull<CFontAtlas>,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    baseline: f32,
}

impl FontAtlas {
    pub fn new(font_size: f32, preferred_family: &str) -> Self {
        requires!(font_size > 0.0);
        let font_size = font_size.max(1.0);
        let preferred_family = CString::new(preferred_family)
            .ok()
            .or_else(|| CString::new(DEFAULT_FONT_FAMILY).ok())
            .expect("CString::new(DEFAULT_FONT_FAMILY) must succeed");
        let raw =
            NonNull::new(unsafe { rudo_font_atlas_new(font_size, preferred_family.as_ptr()) })
                .expect("native FontAtlas init failed");
        let result = Self {
            cell_width: unsafe { rudo_font_atlas_cell_width(raw.as_ptr()) },
            cell_height: unsafe { rudo_font_atlas_cell_height(raw.as_ptr()) },
            baseline: unsafe { rudo_font_atlas_baseline(raw.as_ptr()) },
            raw,
            font_size,
        };
        debug_check_invariant!(result);
        result
    }

    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }

    pub fn baseline(&self) -> f32 {
        self.baseline
    }

    pub fn get_glyph(&mut self, ch: char, bold: bool, italic: bool) -> GlyphInfo {
        unsafe {
            rudo_font_atlas_get_glyph(self.raw.as_ptr(), ch as u32, bold as c_int, italic as c_int)
        }
    }

    pub fn atlas_data(&self) -> &[u8] {
        let mut len = 0usize;
        let ptr = unsafe { rudo_font_atlas_data(self.raw.as_ptr(), &mut len) };
        if ptr.is_null() || len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(ptr, len) }
        }
    }

    pub fn atlas_size(&self) -> (u32, u32) {
        let width = unsafe { rudo_font_atlas_width(self.raw.as_ptr()) };
        let height = unsafe { rudo_font_atlas_height(self.raw.as_ptr()) };
        (
            if width == 0 { ATLAS_WIDTH } else { width },
            if height == 0 { ATLAS_HEIGHT } else { height },
        )
    }
}

impl Drop for FontAtlas {
    fn drop(&mut self) {
        unsafe {
            rudo_font_atlas_free(self.raw.as_ptr());
        }
    }
}

impl CheckInvariant for FontAtlas {
    fn check_invariant(&self) {
        invariant!(
            self.cell_width >= 1.0,
            "FontAtlas: cell_width ({}) < 1.0",
            self.cell_width
        );
        invariant!(
            self.cell_height >= 1.0,
            "FontAtlas: cell_height ({}) < 1.0",
            self.cell_height
        );
        invariant!(
            self.font_size >= 1.0,
            "FontAtlas: font_size ({}) < 1.0",
            self.font_size
        );
    }
}
