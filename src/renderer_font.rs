use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::contracts::CheckInvariant;
use crate::defaults::{APP_NAME, DEFAULT_FONT_FAMILY};

struct FxHasher(u64);
impl std::hash::Hasher for FxHasher {
    #[inline] fn finish(&self) -> u64 { self.0 }
    #[inline] fn write(&mut self, bytes: &[u8]) {
        for &b in bytes { self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517cc1b727220a95); }
    }
}
impl std::hash::BuildHasher for FxHasher {
    type Hasher = FxHasher;
    #[inline] fn build_hasher(&self) -> FxHasher { FxHasher(0) }
}
type FxHashMap<K, V> = HashMap<K, V, FxHasher>;
use crate::fontconfig_ffi as fc;
use crate::freetype_ffi as ft;
use crate::{error_log, info_log, warn_log};

/// Information about a rasterized glyph's position in the atlas and its metrics.
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

const ATLAS_WIDTH: u32 = 1024;
const ATLAS_HEIGHT: u32 = 1024;
const ASCII_RANGE: usize = 95;
const ASCII_STYLES: usize = 4;
const ASCII_CACHE_LEN: usize = ASCII_RANGE * ASCII_STYLES;
const FREETYPE_FIXED_POINT_SCALE: f32 = 64.0;
const FALLBACK_BASELINE_RATIO: f32 = 0.8;
const HOME_ENV_VAR: &str = "HOME";
const XDG_CACHE_HOME_ENV_VAR: &str = "XDG_CACHE_HOME";
const FONT_PLAN_CACHE_DIR: &str = "font-plans-v1";
const FONT_PLAN_STYLE_REGULAR: &str = "regular";
const FONT_PLAN_STYLE_BOLD: &str = "bold";
const FONT_PLAN_STYLE_ITALIC: &str = "italic";
const FONT_PLAN_STYLE_BOLD_ITALIC: &str = "bold-italic";

#[inline]
fn ascii_cache_idx(ch: u8, bold: bool, italic: bool) -> usize {
    requires!((32..=126).contains(&ch), "ascii_cache_idx: ch ({}) not in printable ASCII range 32..=126", ch);
    let style = (bold as usize) * 2 + (italic as usize);
    style * ASCII_RANGE + (ch as usize - 32)
}

#[inline]
fn empty_glyph_info() -> GlyphInfo {
    GlyphInfo {
        u0: 0.0,
        v0: 0.0,
        u1: 0.0,
        v1: 0.0,
        width: 0.0,
        height: 0.0,
        offset_x: 0.0,
        offset_y: 0.0,
    }
}

// ── FreeType font wrapper ────────────────────────────────────────────────────

/// Owns the raw font data and the FT_Face.
struct FtFont {
    face: ft::FT_Face,
    _data: Vec<u8>, // must outlive the face
}

unsafe impl Send for FtFont {}

impl FtFont {
    fn from_bytes(lib: ft::FT_Library, data: Vec<u8>) -> Result<Self, String> {
        requires!(!data.is_empty());
        let fth = ft::ft();
        let mut face: ft::FT_Face = std::ptr::null_mut();
        let err = unsafe {
            (fth.new_memory_face)(lib, data.as_ptr(), data.len() as ft::FT_Long, 0, &mut face)
        };
        if err != 0 || face.is_null() {
            return Err(format!("FT_New_Memory_Face failed: {err}"));
        }
        Ok(Self { face, _data: data })
    }

    fn set_size_px(&self, size_px: f32) {
        requires!(size_px > 0.0);
        let fth = ft::ft();
        let size_px = size_px.max(1.0);
        let size_26_6 = (size_px * FREETYPE_FIXED_POINT_SCALE).round() as ft::FT_F26Dot6;
        unsafe {
            let err = (fth.set_char_size)(self.face, 0, size_26_6, 72, 72);
            if err != 0 {
                (fth.set_pixel_sizes)(self.face, 0, size_px.round().max(1.0) as u32);
            }
        }
    }

    /// Returns (width, height, bitmap_left, bitmap_top, advance_width, bitmap).
    fn rasterize(&self, ch: char) -> (u32, u32, i32, i32, f32, Vec<u8>) {
        let fth = ft::ft();
        let err = unsafe { (fth.load_char)(self.face, ch as ft::FT_ULong, ft::FT_LOAD_RENDER) };
        if err != 0 {
            return (0, 0, 0, 0, 0.0, Vec::new());
        }
        unsafe {
            let slot = &*(*self.face).glyph;
            let bmp = &slot.bitmap;
            let w = bmp.width;
            let h = bmp.rows;
            let advance = slot.metrics.horiAdvance as f32 / FREETYPE_FIXED_POINT_SCALE;

            let pixels = read_bitmap_rows(bmp.buffer, w, h, bmp.pitch);

            (w, h, slot.bitmap_left, slot.bitmap_top, advance, pixels)
        }
    }

    fn has_glyph(&self, ch: char) -> bool {
        let fth = ft::ft();
        unsafe { (fth.get_char_index)(self.face, ch as ft::FT_ULong) != 0 }
    }

    fn glyph_advance(&self, ch: char) -> f32 {
        let fth = ft::ft();
        let err = unsafe { (fth.load_char)(self.face, ch as ft::FT_ULong, ft::FT_LOAD_DEFAULT) };
        if err != 0 { return 0.0; }
        unsafe { (*(*self.face).glyph).metrics.horiAdvance as f32 / FREETYPE_FIXED_POINT_SCALE }
    }

    /// Returns (ascender, descender, line_height) in pixels.
    fn line_metrics(&self) -> (f32, f32, f32) {
        unsafe {
            let sm = &(*(*self.face).size).metrics;
            let asc = sm.ascender as f32 / FREETYPE_FIXED_POINT_SCALE;
            let desc = sm.descender as f32 / FREETYPE_FIXED_POINT_SCALE;
            let height = sm.height as f32 / FREETYPE_FIXED_POINT_SCALE;
            (asc, desc, height)
        }
    }
}

impl Drop for FtFont {
    fn drop(&mut self) {
        let fth = ft::ft();
        unsafe {
            (fth.done_face)(self.face);
        }
    }
}

// ── FreeType library (global) ────────────────────────────────────────────────

fn ft_library() -> ft::FT_Library {
    static LIB: OnceLock<usize> = OnceLock::new();
    let ptr = *LIB.get_or_init(|| {
        let fth = ft::ft();
        let mut lib: ft::FT_Library = std::ptr::null_mut();
        let err = unsafe { (fth.init_freetype)(&mut lib) };
        assert!(err == 0, "FT_Init_FreeType failed: {err}");
        lib as usize
    });
    ptr as ft::FT_Library
}

static FONT_PLAN_CACHE: OnceLock<Mutex<HashMap<String, FontPlan>>> = OnceLock::new();

// ── FontAtlas ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FontPlan {
    regular: PathBuf,
    bold: Option<PathBuf>,
    italic: Option<PathBuf>,
    bold_italic: Option<PathBuf>,
    fallbacks: Vec<PathBuf>,
}

pub struct FontAtlas {
    font_regular: FtFont,
    font_bold: Option<FtFont>,
    font_italic: Option<FtFont>,
    font_bold_italic: Option<FtFont>,
    font_bold_path: Option<PathBuf>,
    font_italic_path: Option<PathBuf>,
    font_bold_italic_path: Option<PathBuf>,
    fallback_fonts: Vec<FtFont>,
    fallback_font_paths: Vec<PathBuf>,
    next_fallback_font: usize,
    #[allow(dead_code)]
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    atlas_data: Vec<u8>,
    cache: FxHashMap<(char, bool, bool), GlyphInfo>,
    current_x: u32,
    current_y: u32,
    row_height: u32,
    dirty: bool,
    ascii_cache: Box<[GlyphInfo; ASCII_CACHE_LEN]>,
    ascii_populated: [bool; ASCII_CACHE_LEN],
}

impl FontAtlas {
    pub fn new(font_size: f32, preferred_family: &str) -> Self {
        requires!(font_size > 0.0);
        let font_plan = load_or_build_font_plan(preferred_family);
        let font_regular = load_primary_font(&font_plan, font_size);
        let cell_width = compute_cell_width(&font_regular);
        let cell_height = compute_cell_height(&font_regular);
        let atlas_data = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT) as usize];

        FontAtlas {
            font_regular,
            font_bold: None,
            font_italic: None,
            font_bold_italic: None,
            font_bold_path: font_plan.bold.filter(|path| path != &font_plan.regular),
            font_italic_path: font_plan.italic.filter(|path| path != &font_plan.regular),
            font_bold_italic_path: font_plan
                .bold_italic
                .filter(|path| path != &font_plan.regular),
            fallback_fonts: Vec::new(),
            fallback_font_paths: font_plan.fallbacks.clone(),
            next_fallback_font: 0,
            font_size,
            cell_width,
            cell_height,
            atlas_data,
            cache: HashMap::with_hasher(FxHasher(0)),
            current_x: 0,
            current_y: 0,
            row_height: 0,
            dirty: false,
            ascii_cache: Box::new([empty_glyph_info(); ASCII_CACHE_LEN]),
            ascii_populated: [false; ASCII_CACHE_LEN],
        }
    }

    pub fn cell_width(&self) -> f32 {
        self.cell_width
    }
    pub fn cell_height(&self) -> f32 {
        self.cell_height
    }

    pub fn baseline(&self) -> f32 {
        let (asc, _, _) = self.font_regular.line_metrics();
        if asc > 0.0 {
            asc
        } else {
            self.cell_height * FALLBACK_BASELINE_RATIO
        }
    }

    pub fn get_glyph(&mut self, ch: char, bold: bool, italic: bool) -> GlyphInfo {
        let code = ch as u32;
        if (32..=126).contains(&code) {
            let idx = ascii_cache_idx(code as u8, bold, italic);
            if !self.ascii_populated[idx] {
                self.ascii_cache[idx] = self.rasterize_glyph(ch, bold, italic);
                self.ascii_populated[idx] = true;
            }
            return self.ascii_cache[idx];
        }

        let key = (ch, bold, italic);
        if let Some(&info) = self.cache.get(&key) {
            return info;
        }

        let info = self.rasterize_glyph(ch, bold, italic);
        self.cache.insert(key, info);
        info
    }

    fn reset_atlas(&mut self) {
        // Glyph bitmaps fully overwrite their atlas regions when rasterized
        // and the renderer only reads within each glyph's UV rect, so zeroing
        // the 1MB atlas buffer is unnecessary. Just reset placement state.
        self.cache.clear();
        self.ascii_populated.fill(false);
        self.current_x = 0;
        self.current_y = 0;
        self.row_height = 0;
        self.dirty = true;
    }

    fn rasterize_glyph(&mut self, ch: char, bold: bool, italic: bool) -> GlyphInfo {
        let font = self.pick_font_with_fallback(ch, bold, italic);
        let (bw, bh, bitmap_left, bitmap_top, _advance, bitmap) = font.rasterize(ch);

        let gw = bw;
        let gh = bh;

        let (ax, ay) = if gw == 0 || gh == 0 {
            (0u32, 0u32)
        } else {
            if self.current_x + gw > ATLAS_WIDTH {
                self.current_y += self.row_height;
                self.current_x = 0;
                self.row_height = 0;
            }
            if self.current_y + gh > ATLAS_HEIGHT {
                self.reset_atlas();
            }

            let ax = self.current_x;
            let ay = self.current_y;
            let atlas_stride = ATLAS_WIDTH as usize;
            let glyph_row_width = gw as usize;
            for row in 0..gh as usize {
                let src_start = row * glyph_row_width;
                let src_end = src_start + glyph_row_width;
                let dst_start = (ay as usize + row) * atlas_stride + ax as usize;
                let dst_end = dst_start + glyph_row_width;
                self.atlas_data[dst_start..dst_end].copy_from_slice(&bitmap[src_start..src_end]);
            }

            self.current_x += gw + 1;
            if gh + 1 > self.row_height {
                self.row_height = gh + 1;
            }
            self.dirty = true;
            (ax, ay)
        };

        let aw = ATLAS_WIDTH as f32;
        let ah = ATLAS_HEIGHT as f32;
        // offset_x/offset_y must match fontdue's Metrics.xmin/ymin semantics:
        //   xmin = horizontal offset from cursor (= FreeType bitmap_left)
        //   ymin = vertical offset of glyph BOTTOM from baseline
        //        = bitmap_top - bitmap_height  (FreeType convention → fontdue convention)
        GlyphInfo {
            u0: ax as f32 / aw,
            v0: ay as f32 / ah,
            u1: (ax + gw) as f32 / aw,
            v1: (ay + gh) as f32 / ah,
            width: gw as f32,
            height: gh as f32,
            offset_x: bitmap_left as f32,
            offset_y: (bitmap_top - gh as i32) as f32,
        }
    }

    pub fn atlas_data(&self) -> &[u8] {
        &self.atlas_data
    }
    pub fn atlas_size(&self) -> (u32, u32) {
        (ATLAS_WIDTH, ATLAS_HEIGHT)
    }

    fn pick_font_with_fallback(&mut self, ch: char, bold: bool, italic: bool) -> &FtFont {
        if (ch as u32) < 128 {
            return self.pick_styled_font(bold, italic);
        }

        let styled_has_glyph = {
            let styled = self.pick_styled_font(bold, italic);
            styled.has_glyph(ch)
        };
        if styled_has_glyph {
            return self.pick_styled_font(bold, italic);
        }
        if self.font_regular.has_glyph(ch) {
            return &self.font_regular;
        }

        for idx in 0..self.fallback_fonts.len() {
            if self.fallback_fonts[idx].has_glyph(ch) {
                return &self.fallback_fonts[idx];
            }
        }

        while self.next_fallback_font < self.fallback_font_paths.len() {
            let path = self.fallback_font_paths[self.next_fallback_font].as_path();
            self.next_fallback_font += 1;

            match load_font_from_path(path, self.font_size) {
                Ok(font) => {
                    info_log!("Fallback font: {}", path.display());
                    let has_glyph = font.has_glyph(ch);
                    self.fallback_fonts.push(font);
                    if has_glyph {
                        let idx = self.fallback_fonts.len() - 1;
                        return &self.fallback_fonts[idx];
                    }
                }
                Err(e) => {
                    warn_log!("Failed to load fallback {}: {}", path.display(), e);
                }
            }
        }

        self.pick_styled_font(bold, italic)
    }

    fn pick_styled_font(&mut self, bold: bool, italic: bool) -> &FtFont {
        match (bold, italic) {
            (true, true) => {
                self.ensure_bold_italic_font();
                self.font_bold_italic
                    .as_ref()
                    .or(self.font_bold.as_ref())
                    .or(self.font_italic.as_ref())
                    .unwrap_or(&self.font_regular)
            }
            (true, false) => {
                self.ensure_bold_font();
                self.font_bold.as_ref().unwrap_or(&self.font_regular)
            }
            (false, true) => {
                self.ensure_italic_font();
                self.font_italic.as_ref().unwrap_or(&self.font_regular)
            }
            (false, false) => &self.font_regular,
        }
    }

    fn ensure_bold_font(&mut self) {
        if self.font_bold.is_none() {
            if let Some(path) = self.font_bold_path.as_deref() {
                self.font_bold = load_optional_style_font(path, self.font_size);
            }
        }
    }

    fn ensure_italic_font(&mut self) {
        if self.font_italic.is_none() {
            if let Some(path) = self.font_italic_path.as_deref() {
                self.font_italic = load_optional_style_font(path, self.font_size);
            }
        }
    }

    fn ensure_bold_italic_font(&mut self) {
        if self.font_bold_italic.is_none() {
            if let Some(path) = self.font_bold_italic_path.as_deref() {
                self.font_bold_italic = load_optional_style_font(path, self.font_size);
            }
        }
    }
}

// ── Font loading helpers ─────────────────────────────────────────────────────

fn load_or_build_font_plan(preferred_family: &str) -> FontPlan {
    let request = normalize_font_request(preferred_family);
    let cache = FONT_PLAN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(plan) = cache
        .lock()
        .expect("font plan cache poisoned")
        .get(&request)
    {
        return plan.clone();
    }

    let plan = read_font_plan_cache(&request)
        .or_else(|| resolve_font_plan_via_fontconfig(&request))
        .or_else(|| {
            warn_log!("Preferred font not found, trying any monospace font via fontconfig.");
            resolve_font_plan_via_fontconfig("monospace")
        })
        .or_else(|| {
            warn_log!("Fontconfig failed, scanning common font directories.");
            scan_filesystem_for_any_font()
        })
        .unwrap_or_else(|| {
            error_log!("No usable system font found.");
            eprintln!(
                "        Install a monospace font or set [font].family in the config (eg. JetBrains Mono, Fira Code, DejaVu Sans Mono, Liberation Mono)."
            );
            std::process::exit(1);
        });

    write_font_plan_cache(&request, &plan);
    cache
        .lock()
        .expect("font plan cache poisoned")
        .insert(request, plan.clone());
    plan
}

/// Last-resort: walk common font directories for any .ttf/.otf file.
fn scan_filesystem_for_any_font() -> Option<FontPlan> {
    const SEARCH_DIRS: &[&str] = &[
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/nix/var/nix/profiles/system/sw/share/X11/fonts",
        "/run/current-system/sw/share/X11/fonts",
    ];

    // Also check $HOME/.local/share/fonts and $HOME/.fonts
    let home = std::env::var("HOME").ok();
    let mut dirs: Vec<PathBuf> = SEARCH_DIRS.iter().map(PathBuf::from).collect();
    if let Some(ref h) = home {
        dirs.push(PathBuf::from(format!("{h}/.local/share/fonts")));
        dirs.push(PathBuf::from(format!("{h}/.fonts")));
    }

    for dir in &dirs {
        if let Some(path) = find_first_font_in(dir) {
            info_log!("Last-resort font: {}", path.display());
            return Some(FontPlan {
                regular: path,
                bold: None,
                italic: None,
                bold_italic: None,
                fallbacks: Vec::new(),
            });
        }
    }
    None
}

fn find_first_font_in(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf") {
                    return Some(path);
                }
            }
        } else if path.is_dir() {
            subdirs.push(path);
        }
    }
    for sub in subdirs {
        if let Some(found) = find_first_font_in(&sub) {
            return Some(found);
        }
    }
    None
}

fn normalize_font_request(preferred_family: &str) -> String {
    let family = preferred_family.trim();
    if family.is_empty() {
        DEFAULT_FONT_FAMILY.to_string()
    } else {
        family.to_string()
    }
}

fn read_font_plan_cache(request: &str) -> Option<FontPlan> {
    let cache_path = font_plan_cache_path(request)?;
    let contents = fs::read_to_string(cache_path).ok()?;

    let mut regular = None;
    let mut bold = None;
    let mut italic = None;
    let mut bold_italic = None;
    let mut fallbacks = Vec::new();

    for line in contents.lines() {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        if value.is_empty() {
            continue;
        }

        match key {
            FONT_PLAN_STYLE_REGULAR => regular = Some(PathBuf::from(value)),
            FONT_PLAN_STYLE_BOLD => bold = Some(PathBuf::from(value)),
            FONT_PLAN_STYLE_ITALIC => italic = Some(PathBuf::from(value)),
            FONT_PLAN_STYLE_BOLD_ITALIC => bold_italic = Some(PathBuf::from(value)),
            "fallback" => fallbacks.push(PathBuf::from(value)),
            _ => {}
        }
    }

    let regular = regular.filter(|path| path.exists())?;
    let bold = bold.filter(|path| path.exists());
    let italic = italic.filter(|path| path.exists());
    let bold_italic = bold_italic.filter(|path| path.exists());
    fallbacks.retain(|path| path.exists());
    dedup_paths(&mut fallbacks);

    Some(FontPlan {
        regular,
        bold,
        italic,
        bold_italic,
        fallbacks,
    })
}

fn write_font_plan_cache(request: &str, plan: &FontPlan) {
    let Some(cache_path) = font_plan_cache_path(request) else {
        return;
    };
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let mut out = String::new();
    out.push_str(FONT_PLAN_STYLE_REGULAR);
    out.push('=');
    out.push_str(&plan.regular.to_string_lossy());
    out.push('\n');

    if let Some(path) = &plan.bold {
        out.push_str(FONT_PLAN_STYLE_BOLD);
        out.push('=');
        out.push_str(&path.to_string_lossy());
        out.push('\n');
    }
    if let Some(path) = &plan.italic {
        out.push_str(FONT_PLAN_STYLE_ITALIC);
        out.push('=');
        out.push_str(&path.to_string_lossy());
        out.push('\n');
    }
    if let Some(path) = &plan.bold_italic {
        out.push_str(FONT_PLAN_STYLE_BOLD_ITALIC);
        out.push('=');
        out.push_str(&path.to_string_lossy());
        out.push('\n');
    }
    for path in &plan.fallbacks {
        out.push_str("fallback=");
        out.push_str(&path.to_string_lossy());
        out.push('\n');
    }

    let _ = fs::write(cache_path, out);
}

fn font_plan_cache_path(request: &str) -> Option<PathBuf> {
    let mut base = if let Ok(path) = std::env::var(XDG_CACHE_HOME_ENV_VAR) {
        if path.is_empty() {
            return None;
        }
        PathBuf::from(path)
    } else {
        let home = std::env::var_os(HOME_ENV_VAR)?;
        PathBuf::from(home).join(".cache")
    };

    base.push(APP_NAME);
    base.push(FONT_PLAN_CACHE_DIR);
    base.push(format!("{}.txt", sanitize_cache_key(request)));
    Some(base)
}

fn sanitize_cache_key(request: &str) -> String {
    let mut out = String::with_capacity(request.len());
    let mut last_was_sep = false;

    for ch in request.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }

    let out = out.trim_matches('_').chars().take(80).collect::<String>();
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

fn resolve_font_plan_via_fontconfig(request: &str) -> Option<FontPlan> {
    let regular = fontconfig_match_file(request)?;
    let bold = fontconfig_match_first_existing(request, &["style=Bold"]);
    let italic = fontconfig_match_first_existing(request, &["style=Italic", "style=Oblique"]);
    let bold_italic = fontconfig_match_first_existing(
        request,
        &[
            "style=Bold Italic",
            "style=Bold Oblique",
            "style=Italic Bold",
        ],
    );

    let mut fallbacks = fontconfig_sort_files(request)?;
    let mut exclude = vec![regular.clone()];
    if let Some(path) = &bold {
        exclude.push(path.clone());
    }
    if let Some(path) = &italic {
        exclude.push(path.clone());
    }
    if let Some(path) = &bold_italic {
        exclude.push(path.clone());
    }
    fallbacks.retain(|path| !exclude.iter().any(|existing| existing == path));
    dedup_paths(&mut fallbacks);

    info_log!("Primary font: {}", regular.display());
    Some(FontPlan {
        regular,
        bold,
        italic,
        bold_italic,
        fallbacks,
    })
}

fn fontconfig_match_first_existing(request: &str, attrs: &[&str]) -> Option<PathBuf> {
    for attr in attrs {
        let pattern = format!("{request}:{attr}");
        if let Some(path) = fontconfig_match_file(&pattern) {
            return Some(path);
        }
    }
    None
}

fn fontconfig_match_file(pattern_str: &str) -> Option<PathBuf> {
    if let Some(fch) = fc::try_fontconfig() {
        unsafe {
            let config = (fch.FcInitLoadConfigAndFonts)();
            if config.is_null() {
                return fontconfig_match_file_command(pattern_str);
            }

            let pattern = match build_fontconfig_pattern(fch, config, pattern_str) {
                Some(pattern) => pattern,
                None => {
                    (fch.FcConfigDestroy)(config);
                    return fontconfig_match_file_command(pattern_str);
                }
            };
            let mut result = fc::FcResultNoMatch;
            let matched = (fch.FcFontMatch)(config, pattern, &mut result);
            let path = if matched.is_null() {
                None
            } else {
                fontconfig_pattern_file(fch, matched)
            };

            if !matched.is_null() {
                (fch.FcPatternDestroy)(matched);
            }
            (fch.FcPatternDestroy)(pattern);
            (fch.FcConfigDestroy)(config);
            return path.or_else(|| fontconfig_match_file_command(pattern_str));
        }
    }

    fontconfig_match_file_command(pattern_str)
}

fn fontconfig_sort_files(pattern_str: &str) -> Option<Vec<PathBuf>> {
    if let Some(fch) = fc::try_fontconfig() {
        unsafe {
            let config = (fch.FcInitLoadConfigAndFonts)();
            if config.is_null() {
                return fontconfig_sort_files_command(pattern_str);
            }

            let pattern = match build_fontconfig_pattern(fch, config, pattern_str) {
                Some(pattern) => pattern,
                None => {
                    (fch.FcConfigDestroy)(config);
                    return fontconfig_sort_files_command(pattern_str);
                }
            };
            let mut result = fc::FcResultNoMatch;
            let set = (fch.FcFontSort)(
                config,
                pattern,
                fc::FcTrue,
                std::ptr::null_mut(),
                &mut result,
            );
            let mut paths = Vec::new();

            if !set.is_null() {
                let font_set = &*set;
                for i in 0..font_set.nfont {
                    let pat = *font_set.fonts.add(i as usize);
                    if let Some(path) = fontconfig_pattern_file(fch, pat) {
                        paths.push(path);
                    }
                }
                (fch.FcFontSetDestroy)(set);
            }

            (fch.FcPatternDestroy)(pattern);
            (fch.FcConfigDestroy)(config);
            dedup_paths(&mut paths);
            return Some(paths);
        }
    }

    fontconfig_sort_files_command(pattern_str)
}

fn fontconfig_match_file_command(pattern: &str) -> Option<PathBuf> {
    let output = Command::new("fc-match")
        .arg("-f")
        .arg("%{file}\n")
        .arg(pattern)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn fontconfig_sort_files_command(pattern: &str) -> Option<Vec<PathBuf>> {
    let output = Command::new("fc-match")
        .arg("-s")
        .arg("-f")
        .arg("%{file}\n")
        .arg(pattern)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut paths = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        if path.exists() {
            paths.push(path);
        }
    }
    dedup_paths(&mut paths);
    Some(paths)
}

unsafe fn build_fontconfig_pattern(
    fch: &fc::FontconfigHandle,
    config: *mut fc::FcConfig,
    pattern: &str,
) -> Option<*mut fc::FcPattern> {
    let pattern = CString::new(pattern).ok()?;
    let pattern = (fch.FcNameParse)(pattern.as_ptr().cast());
    if pattern.is_null() {
        return None;
    }
    let _ = (fch.FcConfigSubstitute)(config, pattern, fc::FcMatchPattern);
    (fch.FcDefaultSubstitute)(pattern);
    Some(pattern)
}

unsafe fn fontconfig_pattern_file(
    fch: &fc::FontconfigHandle,
    pattern: *const fc::FcPattern,
) -> Option<PathBuf> {
    let mut raw = std::ptr::null_mut();
    let result = (fch.FcPatternGetString)(pattern, fc::FC_FILE.as_ptr().cast(), 0, &mut raw);
    if result != fc::FcResultMatch || raw.is_null() {
        return None;
    }
    let path = CStr::from_ptr(raw.cast()).to_string_lossy().into_owned();
    let path = PathBuf::from(path);
    path.exists().then_some(path)
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut i = 0;
    while i < paths.len() {
        if paths[..i].contains(&paths[i]) {
            paths.swap_remove(i);
        } else {
            i += 1;
        }
    }
}

fn load_primary_font(font_plan: &FontPlan, font_size: f32) -> FtFont {
    match load_font_from_path(&font_plan.regular, font_size) {
        Ok(font) => return font,
        Err(e) => {
            warn_log!(
                "Failed to load primary font {}: {}",
                font_plan.regular.display(),
                e
            );
        }
    }

    // Try fallbacks from the plan itself.
    for fb in &font_plan.fallbacks {
        if let Ok(font) = load_font_from_path(fb, font_size) {
            warn_log!("Using fallback font: {}", fb.display());
            return font;
        }
    }

    // Last resort: scan filesystem.
    if let Some(last_resort) = scan_filesystem_for_any_font() {
        if let Ok(font) = load_font_from_path(&last_resort.regular, font_size) {
            warn_log!("Using last-resort font: {}", last_resort.regular.display());
            return font;
        }
    }

    error_log!("No loadable font found — cannot render text.");
    eprintln!(
        "        Install a monospace font or set [font].family in the config (eg. JetBrains Mono, Fira Code, DejaVu Sans Mono, Liberation Mono)."
    );
    std::process::exit(1);
}

fn load_optional_style_font(path: &Path, size_px: f32) -> Option<FtFont> {
    match load_font_from_path(path, size_px) {
        Ok(font) => Some(font),
        Err(e) => {
            warn_log!("Failed to load style font {}: {}", path.display(), e);
            None
        }
    }
}

fn load_font_from_path(path: &Path, size_px: f32) -> Result<FtFont, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let lib = ft_library();
    let font =
        FtFont::from_bytes(lib, bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
    font.set_size_px(size_px);
    Ok(font)
}

fn compute_cell_width(font: &FtFont) -> f32 {
    let samples = ['M', 'W', '@', '0'];
    let mut width = 0.0f32;
    for ch in samples {
        width = width.max(font.glyph_advance(ch));
    }
    width.max(1.0)
}

fn compute_cell_height(font: &FtFont) -> f32 {
    let (asc, desc, line_height) = font.line_metrics();
    let from_metrics = line_height.max(asc - desc);
    if from_metrics > 0.0 {
        from_metrics.max(1.0)
    } else {
        let (_, bh, _, _, _, _) = font.rasterize('M');
        (bh as f32).max(1.0)
    }
}

fn read_bitmap_rows(buffer: *const u8, width: u32, rows: u32, pitch: i32) -> Vec<u8> {
    let mut pixels = vec![0u8; (width * rows) as usize];
    if buffer.is_null() || width == 0 || rows == 0 {
        pixels.clear();
        return pixels;
    }

    let abs_pitch = pitch.unsigned_abs() as usize;
    let row_width = width as usize;
    if abs_pitch < row_width {
        pixels.clear();
        return pixels;
    }

    unsafe {
        if pitch < 0 {
            for row in 0..rows as usize {
                let src = buffer.offset(row as isize * pitch as isize);
                let dst_start = row * row_width;
                let dst_end = dst_start + row_width;
                pixels[dst_start..dst_end]
                    .copy_from_slice(std::slice::from_raw_parts(src, row_width));
            }
        } else {
            for row in 0..rows as usize {
                let src = buffer.add(row * abs_pitch);
                let dst_start = row * row_width;
                let dst_end = dst_start + row_width;
                pixels[dst_start..dst_end]
                    .copy_from_slice(std::slice::from_raw_parts(src, row_width));
            }
        }
    }

    pixels
}

impl CheckInvariant for FontAtlas {
    fn check_invariant(&self) {
        invariant!(self.cell_width >= 1.0, "FontAtlas: cell_width ({}) < 1.0", self.cell_width);
        invariant!(self.cell_height >= 1.0, "FontAtlas: cell_height ({}) < 1.0", self.cell_height);
        invariant!(self.font_size >= 1.0, "FontAtlas: font_size ({}) < 1.0", self.font_size);
    }
}

#[cfg(test)]
mod tests {
    use super::read_bitmap_rows;

    #[test]
    fn read_bitmap_rows_handles_positive_pitch() {
        let buffer = [1u8, 2, 3, 9, 4, 5, 6, 9];
        let pixels = read_bitmap_rows(buffer.as_ptr(), 3, 2, 4);
        assert_eq!(pixels, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn read_bitmap_rows_handles_negative_pitch() {
        let buffer = [4u8, 5, 6, 9, 1, 2, 3, 9];
        let pixels = read_bitmap_rows(buffer.as_ptr().wrapping_add(4), 3, 2, -4);
        assert_eq!(pixels, vec![1, 2, 3, 4, 5, 6]);
    }
}
