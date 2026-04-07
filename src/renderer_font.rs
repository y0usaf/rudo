use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::defaults::DEFAULT_FONT_FAMILY;
use crate::freetype_ffi as ft;

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
const FONT_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];
const HOME_ENV_VAR: &str = "HOME";
const XDG_DATA_HOME_ENV_VAR: &str = "XDG_DATA_HOME";
const XDG_DATA_DIRS_ENV_VAR: &str = "XDG_DATA_DIRS";
const NIX_PROFILES_ENV_VAR: &str = "NIX_PROFILES";
const USER_FONT_SUBDIR: &str = ".local/share/fonts";
const USER_NERD_FONT_SUBDIR: &str = ".nerd-fonts";
const SHARE_FONTS_SUBDIR: &str = "share/fonts";
const NIXOS_SYSTEM_FONT_DIR: &str = "/run/current-system/sw/share/fonts";

#[cfg(target_os = "linux")]
const SYSTEM_FONT_DIRS: &[&str] = &["/usr/share/fonts"];

#[cfg(target_os = "freebsd")]
const SYSTEM_FONT_DIRS: &[&str] = &["/usr/local/share/fonts", "/usr/share/fonts"];

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
const SYSTEM_FONT_DIRS: &[&str] = &[];

#[inline]
fn ascii_cache_idx(ch: u8, bold: bool, italic: bool) -> usize {
    let style = (bold as usize) * 2 + (italic as usize);
    style * ASCII_RANGE + (ch as usize - 32)
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

    fn set_pixel_size(&self, size_px: u32) {
        let fth = ft::ft();
        unsafe {
            (fth.set_pixel_sizes)(self.face, 0, size_px);
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

            let mut pixels = Vec::with_capacity((w * h) as usize);
            if !bmp.buffer.is_null() && w > 0 && h > 0 {
                let pitch = bmp.pitch;
                for row in 0..h {
                    let src = bmp.buffer.offset(row as isize * pitch as isize);
                    pixels.extend_from_slice(std::slice::from_raw_parts(src, w as usize));
                }
            }

            (w, h, slot.bitmap_left, slot.bitmap_top, advance, pixels)
        }
    }

    fn has_glyph(&self, ch: char) -> bool {
        let fth = ft::ft();
        unsafe { (fth.get_char_index)(self.face, ch as ft::FT_ULong) != 0 }
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
    use std::sync::OnceLock;
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

// ── FontAtlas ────────────────────────────────────────────────────────────────

pub struct FontAtlas {
    font_regular: FtFont,
    font_bold: Option<FtFont>,
    font_italic: Option<FtFont>,
    font_bold_italic: Option<FtFont>,
    fallback_fonts: Vec<FtFont>,
    #[allow(dead_code)]
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
    atlas_data: Vec<u8>,
    cache: HashMap<(char, bool, bool), GlyphInfo>,
    current_x: u32,
    current_y: u32,
    row_height: u32,
    dirty: bool,
    ascii_cache: Box<[GlyphInfo; ASCII_CACHE_LEN]>,
    ascii_populated: [bool; ASCII_CACHE_LEN],
}

struct FontCandidate {
    name_fragment: &'static str,
    style: FontStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontAtlas {
    pub fn new(font_size: f32, preferred_family: &str) -> Self {
        let search_roots = build_search_roots();
        let font_files = collect_font_files(&search_roots);
        let (font_regular, font_bold, font_italic, font_bold_italic) =
            load_fonts(&font_files, font_size, preferred_family);
        let fallback_fonts = load_fallback_fonts(&font_files, font_size);
        if !fallback_fonts.is_empty() {
            eprintln!(
                "[INFO] Loaded {} fallback/symbol font(s)",
                fallback_fonts.len()
            );
        }
        let cell_width = compute_cell_width(&font_regular, font_size);
        let cell_height = compute_cell_height(&font_regular);
        let atlas_data = vec![0u8; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize];

        FontAtlas {
            font_regular,
            font_bold,
            font_italic,
            font_bold_italic,
            fallback_fonts,
            font_size,
            cell_width,
            cell_height,
            atlas_data,
            cache: HashMap::new(),
            current_x: 0,
            current_y: 0,
            row_height: 0,
            dirty: false,
            ascii_cache: Box::new(
                [GlyphInfo {
                    u0: 0.0,
                    v0: 0.0,
                    u1: 0.0,
                    v1: 0.0,
                    width: 0.0,
                    height: 0.0,
                    offset_x: 0.0,
                    offset_y: 0.0,
                }; ASCII_CACHE_LEN],
            ),
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

    pub fn get_glyph(&mut self, ch: char, bold: bool, italic: bool) -> &GlyphInfo {
        let code = ch as u32;
        if (32..=126).contains(&code) {
            let idx = ascii_cache_idx(code as u8, bold, italic);
            if self.ascii_populated[idx] {
                return &self.ascii_cache[idx];
            }
            let info = self.rasterize_glyph(ch, bold, italic);
            self.ascii_cache[idx] = info;
            self.ascii_populated[idx] = true;
            return &self.ascii_cache[idx];
        }

        let key = (ch, bold, italic);
        if self.cache.contains_key(&key) {
            return self
                .cache
                .get(&key)
                .expect("glyph present in cache must exist");
        }
        let info = self.rasterize_glyph(ch, bold, italic);
        self.cache.insert(key, info);
        self.cache
            .get(&key)
            .expect("glyph inserted into cache must exist")
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
                self.current_x = 0;
                self.current_y = 0;
                self.row_height = 0;
            }

            let ax = self.current_x;
            let ay = self.current_y;
            for row in 0..gh {
                for col in 0..gw {
                    let src_idx = (row * gw + col) as usize;
                    let alpha = if src_idx < bitmap.len() {
                        bitmap[src_idx]
                    } else {
                        0
                    };
                    let dst_x = ax + col;
                    let dst_y = ay + row;
                    let dst_idx = ((dst_y * ATLAS_WIDTH + dst_x) * 4) as usize;
                    if dst_idx + 3 < self.atlas_data.len() {
                        self.atlas_data[dst_idx] = 255;
                        self.atlas_data[dst_idx + 1] = 255;
                        self.atlas_data[dst_idx + 2] = 255;
                        self.atlas_data[dst_idx + 3] = alpha;
                    }
                }
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
    #[allow(dead_code)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    #[allow(dead_code)]
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    fn pick_font_with_fallback(&self, ch: char, bold: bool, italic: bool) -> &FtFont {
        let primary = self.pick_styled_font(bold, italic);
        if (ch as u32) < 128 {
            return primary;
        }
        if primary.has_glyph(ch) {
            return primary;
        }
        if self.font_regular.has_glyph(ch) {
            return &self.font_regular;
        }
        for fb in &self.fallback_fonts {
            if fb.has_glyph(ch) {
                return fb;
            }
        }
        primary
    }

    fn pick_styled_font(&self, bold: bool, italic: bool) -> &FtFont {
        match (bold, italic) {
            (true, true) => self
                .font_bold_italic
                .as_ref()
                .or(self.font_bold.as_ref())
                .or(self.font_italic.as_ref())
                .unwrap_or(&self.font_regular),
            (true, false) => self.font_bold.as_ref().unwrap_or(&self.font_regular),
            (false, true) => self.font_italic.as_ref().unwrap_or(&self.font_regular),
            (false, false) => &self.font_regular,
        }
    }
}

// ── Font loading helpers ─────────────────────────────────────────────────────

fn build_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for dir in SYSTEM_FONT_DIRS {
        push_unique_path(&mut roots, PathBuf::from(dir));
    }

    if let Some(home) = std::env::var_os(HOME_ENV_VAR) {
        let mut local = PathBuf::from(&home);
        local.push(USER_FONT_SUBDIR);
        push_unique_path(&mut roots, local);

        let mut nerd = PathBuf::from(&home);
        nerd.push(USER_NERD_FONT_SUBDIR);
        push_unique_path(&mut roots, nerd);
    }

    if let Ok(xdg) = std::env::var(XDG_DATA_HOME_ENV_VAR) {
        if !xdg.is_empty() {
            push_unique_path(&mut roots, PathBuf::from(&xdg).join("fonts"));
        }
    }

    if let Ok(xdg_dirs) = std::env::var(XDG_DATA_DIRS_ENV_VAR) {
        for dir in xdg_dirs.split(':').filter(|dir| !dir.is_empty()) {
            push_unique_path(&mut roots, PathBuf::from(dir).join("fonts"));
        }
    }

    if let Ok(nix_profiles) = std::env::var(NIX_PROFILES_ENV_VAR) {
        for profile in nix_profiles.split_whitespace() {
            push_unique_path(&mut roots, PathBuf::from(profile).join(SHARE_FONTS_SUBDIR));
        }
    }

    push_unique_path(&mut roots, PathBuf::from(NIXOS_SYSTEM_FONT_DIR));
    roots
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn load_fonts(
    font_files: &[PathBuf],
    font_size: f32,
    preferred_family: &str,
) -> (FtFont, Option<FtFont>, Option<FtFont>, Option<FtFont>) {
    let px = font_size.round() as u32;

    if let Some(fonts) = load_preferred_family(font_files, px, preferred_family) {
        return fonts;
    }

    let families: &[&[FontCandidate]] = &[
        &[
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFont-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFont-Bold",
                style: FontStyle::Bold,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFont-Italic",
                style: FontStyle::Italic,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFont-BoldItalic",
                style: FontStyle::BoldItalic,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFontMono-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFontMono-Bold",
                style: FontStyle::Bold,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFontMono-Italic",
                style: FontStyle::Italic,
            },
            FontCandidate {
                name_fragment: "JetBrainsMonoNerdFontMono-BoldItalic",
                style: FontStyle::BoldItalic,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "FiraCodeNerdFont-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "FiraCodeNerdFont-Bold",
                style: FontStyle::Bold,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "FiraCodeNerdFontMono-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "FiraCodeNerdFontMono-Bold",
                style: FontStyle::Bold,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "JetBrainsMono-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "JetBrainsMono-Bold",
                style: FontStyle::Bold,
            },
            FontCandidate {
                name_fragment: "JetBrainsMono-Italic",
                style: FontStyle::Italic,
            },
            FontCandidate {
                name_fragment: "JetBrainsMono-BoldItalic",
                style: FontStyle::BoldItalic,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "FiraCode-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "FiraCode-Bold",
                style: FontStyle::Bold,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "DejaVuSansMono",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "DejaVuSansMono-Bold",
                style: FontStyle::Bold,
            },
            FontCandidate {
                name_fragment: "DejaVuSansMono-Oblique",
                style: FontStyle::Italic,
            },
            FontCandidate {
                name_fragment: "DejaVuSansMono-BoldOblique",
                style: FontStyle::BoldItalic,
            },
        ],
        &[
            FontCandidate {
                name_fragment: "LiberationMono-Regular",
                style: FontStyle::Regular,
            },
            FontCandidate {
                name_fragment: "LiberationMono-Bold",
                style: FontStyle::Bold,
            },
            FontCandidate {
                name_fragment: "LiberationMono-Italic",
                style: FontStyle::Italic,
            },
            FontCandidate {
                name_fragment: "LiberationMono-BoldItalic",
                style: FontStyle::BoldItalic,
            },
        ],
    ];

    for family in families {
        let Some(regular_candidate) = family.iter().find(|c| c.style == FontStyle::Regular) else {
            continue;
        };
        if let Some(regular_path) = find_font_file(font_files, regular_candidate.name_fragment) {
            if let Ok(regular_font) = load_font_from_path(&regular_path, px) {
                eprintln!("[INFO] Primary font: {}", regular_path.display());
                let bold = family
                    .iter()
                    .find(|c| c.style == FontStyle::Bold)
                    .and_then(|c| find_font_file(font_files, c.name_fragment))
                    .and_then(|p| load_font_from_path(&p, px).ok());
                let italic = family
                    .iter()
                    .find(|c| c.style == FontStyle::Italic)
                    .and_then(|c| find_font_file(font_files, c.name_fragment))
                    .and_then(|p| load_font_from_path(&p, px).ok());
                let bold_italic = family
                    .iter()
                    .find(|c| c.style == FontStyle::BoldItalic)
                    .and_then(|c| find_font_file(font_files, c.name_fragment))
                    .and_then(|p| load_font_from_path(&p, px).ok());
                return (regular_font, bold, italic, bold_italic);
            }
        }
    }

    eprintln!("[FATAL] No usable system monospace font found.");
    eprintln!(
        "        Install a monospace font or set [font].family in the config (eg. JetBrains Mono, Fira Code, DejaVu Sans Mono, Liberation Mono)."
    );
    std::process::exit(1);
}

fn load_preferred_family(
    font_files: &[PathBuf],
    size_px: u32,
    preferred_family: &str,
) -> Option<(FtFont, Option<FtFont>, Option<FtFont>, Option<FtFont>)> {
    let family = preferred_family.trim();
    if family.is_empty() || family.eq_ignore_ascii_case(DEFAULT_FONT_FAMILY) {
        return None;
    }

    let regular_path = find_font_file_by_style(font_files, family, FontStyle::Regular)?;
    let regular_font = load_font_from_path(&regular_path, size_px).ok()?;
    eprintln!(
        "[INFO] Primary font (configured): {}",
        regular_path.display()
    );

    let bold = find_font_file_by_style(font_files, family, FontStyle::Bold)
        .and_then(|path| load_font_from_path(&path, size_px).ok());
    let italic = find_font_file_by_style(font_files, family, FontStyle::Italic)
        .and_then(|path| load_font_from_path(&path, size_px).ok());
    let bold_italic = find_font_file_by_style(font_files, family, FontStyle::BoldItalic)
        .and_then(|path| load_font_from_path(&path, size_px).ok());

    Some((regular_font, bold, italic, bold_italic))
}

fn find_font_file_by_style(files: &[PathBuf], family: &str, style: FontStyle) -> Option<PathBuf> {
    let family = normalize_font_name(family);
    files
        .iter()
        .find(|path| {
            let name = normalize_path_file_name(path);
            name.contains(&family) && style_matches(path, style)
        })
        .cloned()
}

fn normalize_font_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn normalize_path_file_name(path: &Path) -> String {
    normalize_font_name(&path.file_name().unwrap_or_default().to_string_lossy())
}

fn style_matches(path: &Path, style: FontStyle) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let has_bold = name.contains("bold");
    let has_italic = name.contains("italic") || name.contains("oblique");

    match style {
        FontStyle::Regular => !has_bold && !has_italic,
        FontStyle::Bold => has_bold && !has_italic,
        FontStyle::Italic => !has_bold && has_italic,
        FontStyle::BoldItalic => has_bold && has_italic,
    }
}

fn load_fallback_fonts(font_files: &[PathBuf], font_size: f32) -> Vec<FtFont> {
    let fallback_fragments: &[&str] = &[
        "SymbolsNerdFontMono-Regular",
        "SymbolsNerdFont-Regular",
        "NerdFontsSymbols",
        "PowerlineSymbols",
        "FontAwesome",
        "fa-solid",
        "fa-brands",
        "fa-regular",
        "NotoColorEmoji",
        "NotoEmoji",
        "NotoSansSymbols2",
        "NotoSansSymbols-",
        "NotoSansMono",
        "MaterialDesignIcons",
        "MaterialIcons",
        "codicon",
        "DejaVuSans.",
        "Unifont",
    ];

    let px = font_size.round() as u32;
    let mut loaded: Vec<FtFont> = Vec::new();
    let mut loaded_paths: Vec<String> = Vec::new();

    for fragment in fallback_fragments {
        let needle = fragment.to_ascii_lowercase();
        for path in font_files {
            let fname = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if !fname.contains(&needle) {
                continue;
            }
            let display = path.display().to_string();
            if loaded_paths.contains(&display) {
                continue;
            }
            match load_font_from_path(path, px) {
                Ok(font) => {
                    eprintln!("[INFO] Fallback font: {}", display);
                    loaded_paths.push(display);
                    loaded.push(font);
                    break;
                }
                Err(e) => {
                    eprintln!("[WARN] Failed to load fallback {}: {}", display, e);
                }
            }
        }
    }

    loaded
}

fn collect_font_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        collect_font_files_rec(root, &mut out);
    }
    out
}

fn collect_font_files_rec(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.is_file() {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if FONT_EXTENSIONS.contains(&ext.as_str()) {
                out.push(path.to_path_buf());
            }
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_font_files_rec(&entry.path(), out);
    }
}

fn find_font_file(files: &[PathBuf], name_fragment: &str) -> Option<PathBuf> {
    let needle = name_fragment.to_ascii_lowercase();
    files
        .iter()
        .find(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains(&needle)
        })
        .cloned()
}

fn load_font_from_path(path: &Path, size_px: u32) -> Result<FtFont, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let lib = ft_library();
    let font =
        FtFont::from_bytes(lib, bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
    font.set_pixel_size(size_px);
    Ok(font)
}

fn compute_cell_width(font: &FtFont, _font_size: f32) -> f32 {
    let samples = ['M', 'W', '@', '0'];
    let mut width = 0.0f32;
    for ch in samples {
        let (bw, _, _, _, advance, _) = font.rasterize(ch);
        width = width.max(advance.max(bw as f32));
    }
    width.ceil().max(1.0)
}

fn compute_cell_height(font: &FtFont) -> f32 {
    let (asc, desc, line_height) = font.line_metrics();
    let from_metrics = line_height.max(asc - desc);
    if from_metrics > 0.0 {
        from_metrics.ceil().max(1.0)
    } else {
        let (_, bh, _, _, _, _) = font.rasterize('M');
        (bh as f32).ceil().max(1.0)
    }
}
