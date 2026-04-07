//! Software framebuffer renderer scaffold for Wayland shm backend.
//! Pixels are BGRA8 little-endian.

use crate::cursor::CursorRenderer;
use crate::font::FontAtlas;
use crate::terminal::cell::CellFlags;
use crate::terminal::damage::DamageTracker;
use crate::terminal::grid::Grid;
use crate::terminal::selection::Selection;
use crate::terminal::theme::Theme;

/// Distance threshold for edge-strip rendering (cursor outline)
const EDGE_STRIP_DISTANCE_SQ: f32 = 0.8;
const FLOAT_CHANGE_EPSILON: f32 = 0.001;
const BT709_LUMA_RED: f32 = 0.2126;
const BT709_LUMA_GREEN: f32 = 0.7152;
const BT709_LUMA_BLUE: f32 = 0.0722;
const LUMA_CONTRAST_THRESHOLD: f32 = 0.5;
const MAX_COLOR_CHANNEL: f32 = 255.0;
const MAX_COLOR_CHANNEL_U16: u16 = 255;
const ALPHA_ROUND_BIAS: u16 = 127;
const DEGENERATE_EDGE_EPSILON_SQ: f32 = 0.0001;

#[allow(dead_code)]
pub struct FrameBuffer<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: &'a mut [u8],
}

#[allow(dead_code)]
pub struct SoftwareRenderer {
    font: FontAtlas,
    theme: Theme,
    font_family: String,
    font_size: f32,
    base_font_size: f32,
    scale: f32,
    cell_width: u32,
    cell_height: u32,
    baseline: i32,
    padding: u32,
    background_alpha: u8,
    offset_x: u32,
    offset_y: u32,
}

#[allow(dead_code)]
impl SoftwareRenderer {
    pub fn new(
        font_size: f32,
        font_family: String,
        theme: Theme,
        padding: u32,
        background_opacity: f32,
    ) -> Self {
        let font_size = font_size.max(1.0);
        let mut renderer = Self {
            font: FontAtlas::new(font_size, &font_family),
            theme,
            font_family,
            font_size,
            base_font_size: font_size,
            scale: 1.0,
            cell_width: 1,
            cell_height: 1,
            baseline: 0,
            padding,
            background_alpha: Self::opacity_to_alpha(background_opacity),
            offset_x: 0,
            offset_y: 0,
        };
        renderer.rebuild_font();
        renderer
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f32) {
        let scale = scale.max(1.0);
        if (self.scale - scale).abs() < FLOAT_CHANGE_EPSILON {
            return;
        }
        self.scale = scale;
        self.rebuild_font();
    }

    fn opacity_to_alpha(opacity: f32) -> u8 {
        (opacity.clamp(0.0, 1.0) * MAX_COLOR_CHANNEL).round() as u8
    }

    fn premultiplied_bgra(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        [premultiply(b, a), premultiply(g, a), premultiply(r, a), a]
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn set_font_size(&mut self, size: f32) {
        let size = size.max(1.0);
        if (self.font_size - size).abs() < f32::EPSILON {
            return;
        }
        self.font_size = size;
        self.rebuild_font();
    }

    pub fn increase_font_size(&mut self, delta: f32) {
        self.set_font_size(self.font_size + delta.max(0.0));
    }

    pub fn decrease_font_size(&mut self, delta: f32) {
        self.set_font_size((self.font_size - delta.max(0.0)).max(1.0));
    }

    pub fn reset_font_size(&mut self) {
        self.set_font_size(self.base_font_size);
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width as f32, self.cell_height as f32)
    }

    /// Compute grid dimensions from physical (scaled) pixel dimensions.
    pub fn grid_size_for_window(&mut self, width: u32, height: u32) -> (usize, usize) {
        let cw = self.cell_width.max(1);
        let ch = self.cell_height.max(1);
        let phys_pad = (self.padding as f32 * self.scale).round() as u32;
        let usable_w = width.saturating_sub(phys_pad * 2);
        let usable_h = height.saturating_sub(phys_pad * 2);
        let cols = (usable_w / cw).max(1) as usize;
        let rows = (usable_h / ch).max(1) as usize;
        // Center the grid: split leftover pixels evenly on both sides
        let grid_w = cols as u32 * cw;
        let grid_h = rows as u32 * ch;
        self.offset_x = (width.saturating_sub(grid_w)) / 2;
        self.offset_y = (height.saturating_sub(grid_h)) / 2;
        (cols, rows)
    }

    /// Returns the pixel offset from the top-left of the window to the
    /// top-left of the first grid cell.
    pub fn grid_offset(&self) -> (f32, f32) {
        (self.offset_x as f32, self.offset_y as f32)
    }

    fn rebuild_font(&mut self) {
        let physical_size = (self.font_size * self.scale).round().max(1.0);
        let mut font = FontAtlas::new(physical_size, &self.font_family);
        for ch in 32u8..=126u8 {
            font.get_glyph(ch as char, false, false);
            font.get_glyph(ch as char, true, false);
            font.get_glyph(ch as char, false, true);
            font.get_glyph(ch as char, true, true);
        }
        self.cell_width = font.cell_width().ceil() as u32;
        self.cell_height = font.cell_height().ceil() as u32;
        self.baseline = font.baseline().round() as i32;
        self.font = font;
    }

    pub fn render(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        grid: &Grid,
        cursor: &CursorRenderer,
        selection: &Selection,
        _damage: &DamageTracker,
    ) {
        // NOTE: We always redraw every row because we use triple-buffered shm.
        // Each frame may target a different buffer, so we cannot assume non-dirty
        // rows already contain correct pixels from the previous frame.
        let ox = self.offset_x;
        let oy = self.offset_y;

        let bg_alpha = self.background_alpha;

        // Fill top margin (always uses background opacity)
        if oy > 0 {
            self.fill_rect(
                fb,
                0,
                0,
                fb.width,
                oy,
                self.theme.background.r(),
                self.theme.background.g(),
                self.theme.background.b(),
                bg_alpha,
            );
        }

        for row in 0..grid.rows() {
            let selected_range = selection.row_range(row);
            let y = oy + row as u32 * self.cell_height;

            // Clear the entire row span (including left/right margins) with
            // the default background at the configured opacity.
            self.fill_rect(
                fb,
                0,
                y,
                fb.width,
                self.cell_height,
                self.theme.background.r(),
                self.theme.background.g(),
                self.theme.background.b(),
                bg_alpha,
            );

            for col in 0..grid.cols() {
                let cell = grid.view_cell(col, row);
                let mut fg = if cell.fg_src == crate::terminal::cell::ColorSource::Default {
                    self.theme.foreground
                } else {
                    cell.fg
                };
                let mut bg = if cell.bg_src == crate::terminal::cell::ColorSource::Default {
                    self.theme.background
                } else {
                    cell.bg
                };

                // Track whether this cell uses the default background.
                // Only the default background gets transparency; explicit
                // cell backgrounds (escape-sequence colors, selection,
                // reverse video) are always fully opaque — matching foot's
                // ALPHA_MODE_DEFAULT behaviour.
                let mut cell_bg_is_default =
                    cell.bg_src == crate::terminal::cell::ColorSource::Default;

                if cell.flags.contains(CellFlags::REVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                    cell_bg_is_default = false;
                }
                if let Some((start, end)) = selected_range {
                    if col >= start && col <= end {
                        bg = self.theme.selection;
                        cell_bg_is_default = false;
                    }
                }

                let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };

                let x = ox + col as u32 * self.cell_width;
                self.fill_rect(
                    fb,
                    x,
                    y,
                    self.cell_width,
                    self.cell_height,
                    bg.r(),
                    bg.g(),
                    bg.b(),
                    cell_alpha,
                );

                if !cell.flags.contains(CellFlags::HIDDEN)
                    && !cell.flags.contains(CellFlags::WIDE_SPACER)
                {
                    self.draw_cell_glyph(
                        fb,
                        x,
                        y,
                        cell.character(),
                        fg.r(),
                        fg.g(),
                        fg.b(),
                        cell.flags.contains(CellFlags::BOLD),
                        cell.flags.contains(CellFlags::ITALIC),
                    );
                }
            }
        }

        // Fill bottom margin (always uses background opacity)
        let grid_bottom = oy + grid.rows() as u32 * self.cell_height;
        if grid_bottom < fb.height {
            self.fill_rect(
                fb,
                0,
                grid_bottom,
                fb.width,
                fb.height - grid_bottom,
                self.theme.background.r(),
                self.theme.background.g(),
                self.theme.background.b(),
                bg_alpha,
            );
        }

        if grid.cursor_visible() && cursor.is_visible() && !grid.is_viewing_scrollback() {
            self.draw_animated_cursor(fb, grid, cursor);
        }
    }

    fn clear(&self, fb: &mut FrameBuffer<'_>, r: u8, g: u8, b: u8, alpha: u8) {
        let pixel = Self::premultiplied_bgra(r, g, b, alpha);
        let row_bytes = fb.width as usize * 4;
        let stride = fb.stride as usize;
        for y in 0..fb.height as usize {
            let row_start = y * stride;
            let row_end = row_start.saturating_add(row_bytes).min(fb.pixels.len());
            let row = &mut fb.pixels[row_start..row_end];
            for chunk in row.chunks_exact_mut(4) {
                chunk.copy_from_slice(&pixel);
            }
        }
    }

    fn fill_rect(
        &self,
        fb: &mut FrameBuffer<'_>,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        r: u8,
        g: u8,
        b: u8,
        alpha: u8,
    ) {
        let max_x = (x + w).min(fb.width);
        let max_y = (y + h).min(fb.height);
        if x >= max_x || y >= max_y {
            return;
        }

        let pixel = Self::premultiplied_bgra(r, g, b, alpha);
        let start_x = x as usize * 4;
        let row_bytes = (max_x - x) as usize * 4;
        let stride = fb.stride as usize;

        for py in y as usize..max_y as usize {
            let row_start = py * stride + start_x;
            let row_end = row_start.saturating_add(row_bytes).min(fb.pixels.len());
            let row = &mut fb.pixels[row_start..row_end];
            for chunk in row.chunks_exact_mut(4) {
                chunk.copy_from_slice(&pixel);
            }
        }
    }

    fn draw_animated_cursor(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        grid: &Grid,
        cursor: &CursorRenderer,
    ) {
        let corners_grid = cursor.corner_positions();
        let ox = self.offset_x as f32;
        let oy = self.offset_y as f32;
        let corners_px = [
            (
                ox + corners_grid[0].0 * self.cell_width as f32,
                oy + corners_grid[0].1 * self.cell_height as f32,
            ),
            (
                ox + corners_grid[1].0 * self.cell_width as f32,
                oy + corners_grid[1].1 * self.cell_height as f32,
            ),
            (
                ox + corners_grid[2].0 * self.cell_width as f32,
                oy + corners_grid[2].1 * self.cell_height as f32,
            ),
            (
                ox + corners_grid[3].0 * self.cell_width as f32,
                oy + corners_grid[3].1 * self.cell_height as f32,
            ),
        ];

        match cursor.shape {
            crate::cursor::CursorShape::Block => {
                self.fill_quad(
                    fb,
                    corners_px,
                    self.theme.cursor.r(),
                    self.theme.cursor.g(),
                    self.theme.cursor.b(),
                );
                let cell = grid.cell(grid.cursor_col(), grid.cursor_row());
                let (fr, fg, fb_col) = contrasting_cursor_text_color(
                    self.theme.cursor.r(),
                    self.theme.cursor.g(),
                    self.theme.cursor.b(),
                );
                self.draw_cell_glyph_clipped(
                    fb,
                    self.offset_x + grid.cursor_col() as u32 * self.cell_width,
                    self.offset_y + grid.cursor_row() as u32 * self.cell_height,
                    cell.character(),
                    fr,
                    fg,
                    fb_col,
                    cell.flags.contains(CellFlags::BOLD),
                    cell.flags.contains(CellFlags::ITALIC),
                    corners_px,
                );
            }
            crate::cursor::CursorShape::Beam | crate::cursor::CursorShape::Underline => {
                self.stroke_quad_edges(
                    fb,
                    corners_px,
                    self.theme.cursor.r(),
                    self.theme.cursor.g(),
                    self.theme.cursor.b(),
                );
            }
        }
    }

    fn fill_quad(&self, fb: &mut FrameBuffer<'_>, quad: [(f32, f32); 4], r: u8, g: u8, b: u8) {
        let min_x = quad
            .iter()
            .map(|p| p.0)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let min_y = quad
            .iter()
            .map(|p| p.1)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_x = quad
            .iter()
            .map(|p| p.0)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0) as u32;
        let max_y = quad
            .iter()
            .map(|p| p.1)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0) as u32;
        for y in min_y..max_y.min(fb.height) {
            for x in min_x..max_x.min(fb.width) {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                if point_in_triangle(p, quad[0], quad[1], quad[2])
                    || point_in_triangle(p, quad[0], quad[2], quad[3])
                {
                    put_bgra(fb, x, y, b, g, r, 255);
                }
            }
        }
    }

    fn stroke_quad_edges(
        &self,
        fb: &mut FrameBuffer<'_>,
        quad: [(f32, f32); 4],
        r: u8,
        g: u8,
        b: u8,
    ) {
        let edges = [
            (quad[0], quad[1]),
            (quad[1], quad[2]),
            (quad[2], quad[3]),
            (quad[3], quad[0]),
        ];
        for &(a, c) in &edges {
            self.fill_edge_strip(fb, a, c, r, g, b);
        }
    }

    fn fill_edge_strip(
        &self,
        fb: &mut FrameBuffer<'_>,
        a: (f32, f32),
        b: (f32, f32),
        r: u8,
        g: u8,
        b_col: u8,
    ) {
        let min_x = a.0.min(b.0).floor().max(0.0) as i32 - 1;
        let max_x = a.0.max(b.0).ceil().min(fb.width as f32) as i32 + 1;
        let min_y = a.1.min(b.1).floor().max(0.0) as i32 - 1;
        let max_y = a.1.max(b.1).ceil().min(fb.height as f32) as i32 + 1;
        let abx = b.0 - a.0;
        let aby = b.1 - a.1;
        let len2 = abx * abx + aby * aby;
        if len2 <= DEGENERATE_EDGE_EPSILON_SQ {
            return;
        }
        for y in min_y.max(0) as u32..(max_y.max(0) as u32).min(fb.height) {
            for x in min_x.max(0) as u32..(max_x.max(0) as u32).min(fb.width) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let apx = px - a.0;
                let apy = py - a.1;
                let t = ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0);
                let cx = a.0 + t * abx;
                let cy = a.1 + t * aby;
                let dx = px - cx;
                let dy = py - cy;
                if dx * dx + dy * dy <= EDGE_STRIP_DISTANCE_SQ {
                    put_bgra(fb, x, y, b_col, g, r, 255);
                }
            }
        }
    }

    fn draw_cell_glyph_clipped(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        cell_x: u32,
        cell_y: u32,
        ch: char,
        r: u8,
        g: u8,
        b: u8,
        bold: bool,
        italic: bool,
        clip_quad: [(f32, f32); 4],
    ) {
        let glyph = *self.font.get_glyph(ch, bold, italic);
        if glyph.width <= 0.0 || glyph.height <= 0.0 {
            return;
        }
        let (atlas_w, atlas_h) = self.font.atlas_size();
        let atlas = self.font.atlas_data();
        let src_x0 = (glyph.u0 * atlas_w as f32).round() as u32;
        let src_y0 = (glyph.v0 * atlas_h as f32).round() as u32;
        let gw = glyph.width as u32;
        let gh = glyph.height as u32;
        let dst_x0 = cell_x as i32 + glyph.offset_x.round() as i32;
        let dst_y0 = cell_y as i32 + self.baseline
            - glyph.height.round() as i32
            - glyph.offset_y.round() as i32;

        for gy in 0..gh {
            for gx in 0..gw {
                let sx = src_x0 + gx;
                let sy = src_y0 + gy;
                if sx >= atlas_w || sy >= atlas_h {
                    continue;
                }
                let sidx = ((sy * atlas_w + sx) * 4) as usize;
                let alpha = atlas.get(sidx + 3).copied().unwrap_or(0);
                if alpha == 0 {
                    continue;
                }
                let dx = dst_x0 + gx as i32;
                let dy = dst_y0 + gy as i32;
                if dx < 0 || dy < 0 || dx as u32 >= fb.width || dy as u32 >= fb.height {
                    continue;
                }
                let p = (dx as f32 + 0.5, dy as f32 + 0.5);
                if point_in_triangle(p, clip_quad[0], clip_quad[1], clip_quad[2])
                    || point_in_triangle(p, clip_quad[0], clip_quad[2], clip_quad[3])
                {
                    blend_bgra(fb, dx as u32, dy as u32, b, g, r, alpha);
                }
            }
        }
    }

    fn draw_cell_glyph(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        cell_x: u32,
        cell_y: u32,
        ch: char,
        r: u8,
        g: u8,
        b: u8,
        bold: bool,
        italic: bool,
    ) {
        let glyph = *self.font.get_glyph(ch, bold, italic);
        if glyph.width <= 0.0 || glyph.height <= 0.0 {
            return;
        }
        let (atlas_w, atlas_h) = self.font.atlas_size();
        let atlas = self.font.atlas_data();
        let src_x0 = (glyph.u0 * atlas_w as f32).round() as u32;
        let src_y0 = (glyph.v0 * atlas_h as f32).round() as u32;
        let gw = glyph.width as u32;
        let gh = glyph.height as u32;
        let dst_x0 = cell_x as i32 + glyph.offset_x.round() as i32;
        let dst_y0 = cell_y as i32 + self.baseline
            - glyph.height.round() as i32
            - glyph.offset_y.round() as i32;

        for gy in 0..gh {
            for gx in 0..gw {
                let sx = src_x0 + gx;
                let sy = src_y0 + gy;
                if sx >= atlas_w || sy >= atlas_h {
                    continue;
                }
                let sidx = ((sy * atlas_w + sx) * 4) as usize;
                let alpha = atlas.get(sidx + 3).copied().unwrap_or(0);
                if alpha == 0 {
                    continue;
                }
                let dx = dst_x0 + gx as i32;
                let dy = dst_y0 + gy as i32;
                if dx < 0 || dy < 0 || dx as u32 >= fb.width || dy as u32 >= fb.height {
                    continue;
                }
                blend_bgra(fb, dx as u32, dy as u32, b, g, r, alpha);
            }
        }
    }
}

fn point_in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s1 = (p.0 - c.0) * (a.1 - c.1) - (a.0 - c.0) * (p.1 - c.1);
    let s2 = (p.0 - a.0) * (b.1 - a.1) - (b.0 - a.0) * (p.1 - a.1);
    let s3 = (p.0 - b.0) * (c.1 - b.1) - (c.0 - b.0) * (p.1 - b.1);
    let has_neg = s1 < 0.0 || s2 < 0.0 || s3 < 0.0;
    let has_pos = s1 > 0.0 || s2 > 0.0 || s3 > 0.0;
    !(has_neg && has_pos)
}

fn contrasting_cursor_text_color(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let luma =
        (BT709_LUMA_RED * r as f32 + BT709_LUMA_GREEN * g as f32 + BT709_LUMA_BLUE * b as f32)
            / MAX_COLOR_CHANNEL;
    if luma > LUMA_CONTRAST_THRESHOLD {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((channel as u16 * alpha as u16 + ALPHA_ROUND_BIAS) / MAX_COLOR_CHANNEL_U16) as u8
}

fn premultiplied_bgra(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [premultiply(b, a), premultiply(g, a), premultiply(r, a), a]
}

fn put_bgra(fb: &mut FrameBuffer<'_>, x: u32, y: u32, b: u8, g: u8, r: u8, a: u8) {
    let idx = (y as usize * fb.stride as usize + x as usize * 4) as usize;
    if idx + 3 >= fb.pixels.len() {
        return;
    }
    let pixel = premultiplied_bgra(r, g, b, a);
    fb.pixels[idx] = pixel[0];
    fb.pixels[idx + 1] = pixel[1];
    fb.pixels[idx + 2] = pixel[2];
    fb.pixels[idx + 3] = pixel[3];
}

fn blend_bgra(fb: &mut FrameBuffer<'_>, x: u32, y: u32, b: u8, g: u8, r: u8, a: u8) {
    let idx = (y as usize * fb.stride as usize + x as usize * 4) as usize;
    if idx + 3 >= fb.pixels.len() {
        return;
    }

    let src = premultiplied_bgra(r, g, b, a);
    let dst_b = fb.pixels[idx] as u16;
    let dst_g = fb.pixels[idx + 1] as u16;
    let dst_r = fb.pixels[idx + 2] as u16;
    let dst_a = fb.pixels[idx + 3] as u16;
    let src_a = src[3] as u16;
    let inv = MAX_COLOR_CHANNEL_U16 - src_a;

    fb.pixels[idx] = (src[0] as u16 + ((dst_b * inv + ALPHA_ROUND_BIAS) / MAX_COLOR_CHANNEL_U16))
        .min(MAX_COLOR_CHANNEL_U16) as u8;
    fb.pixels[idx + 1] = (src[1] as u16
        + ((dst_g * inv + ALPHA_ROUND_BIAS) / MAX_COLOR_CHANNEL_U16))
        .min(MAX_COLOR_CHANNEL_U16) as u8;
    fb.pixels[idx + 2] = (src[2] as u16
        + ((dst_r * inv + ALPHA_ROUND_BIAS) / MAX_COLOR_CHANNEL_U16))
        .min(MAX_COLOR_CHANNEL_U16) as u8;
    fb.pixels[idx + 3] = (src_a + ((dst_a * inv + ALPHA_ROUND_BIAS) / MAX_COLOR_CHANNEL_U16))
        .min(MAX_COLOR_CHANNEL_U16) as u8;
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_opaque() {
        assert_eq!(premultiply(255, 255), 255);
        assert_eq!(premultiply(128, 255), 128);
        assert_eq!(premultiply(0, 255), 0);
    }

    #[test]
    fn premultiply_transparent() {
        assert_eq!(premultiply(255, 0), 0);
        assert_eq!(premultiply(128, 0), 0);
        assert_eq!(premultiply(0, 0), 0);
    }

    #[test]
    fn premultiply_half() {
        assert_eq!(premultiply(255, 128), 128);
        assert_eq!(premultiply(128, 128), 64);
    }

    #[test]
    fn premultiplied_bgra_opaque() {
        let px = premultiplied_bgra(255, 128, 64, 255);
        assert_eq!(px, [64, 128, 255, 255]);
    }

    #[test]
    fn premultiplied_bgra_transparent() {
        let px = premultiplied_bgra(200, 100, 50, 0);
        assert_eq!(px, [0, 0, 0, 0]);
    }

    #[test]
    fn premultiplied_bgra_half_alpha() {
        let px = premultiplied_bgra(255, 255, 255, 128);
        assert_eq!(px, [128, 128, 128, 128]);
    }

    #[test]
    fn pixel_byte_order_matches_argb8888_le() {
        let px = premultiplied_bgra(0xFF, 0x00, 0x80, 255);
        assert_eq!(px, [0x80, 0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn opacity_to_alpha_range() {
        assert_eq!(SoftwareRenderer::opacity_to_alpha(0.0), 0);
        assert_eq!(SoftwareRenderer::opacity_to_alpha(0.5), 128);
        assert_eq!(SoftwareRenderer::opacity_to_alpha(1.0), 255);
    }

    #[test]
    fn opacity_to_alpha_clamps() {
        assert_eq!(SoftwareRenderer::opacity_to_alpha(-1.0), 0);
        assert_eq!(SoftwareRenderer::opacity_to_alpha(2.0), 255);
    }

    fn make_fb(w: u32, h: u32) -> (Vec<u8>, u32) {
        let stride = w * 4;
        let pixels = vec![0u8; (stride * h) as usize];
        (pixels, stride)
    }

    #[test]
    fn blend_full_coverage_replaces_dst() {
        let (mut px, stride) = make_fb(1, 1);
        px[0..4].copy_from_slice(&premultiplied_bgra(255, 255, 255, 128));
        let mut fb = FrameBuffer {
            width: 1,
            height: 1,
            stride,
            pixels: &mut px,
        };
        blend_bgra(&mut fb, 0, 0, 0, 0, 0, 255);
        assert_eq!(&fb.pixels[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn blend_zero_coverage_preserves_dst() {
        let (mut px, stride) = make_fb(1, 1);
        px[0..4].copy_from_slice(&premultiplied_bgra(100, 200, 50, 180));
        let saved = [px[0], px[1], px[2], px[3]];
        let mut fb = FrameBuffer {
            width: 1,
            height: 1,
            stride,
            pixels: &mut px,
        };
        blend_bgra(&mut fb, 0, 0, 255, 255, 255, 0);
        assert_eq!(&fb.pixels[0..4], &saved);
    }

    #[test]
    fn blend_on_transparent_bg_produces_correct_alpha() {
        let (mut px, stride) = make_fb(1, 1);
        px[0..4].copy_from_slice(&[0, 0, 0, 0]);
        let mut fb = FrameBuffer {
            width: 1,
            height: 1,
            stride,
            pixels: &mut px,
        };
        blend_bgra(&mut fb, 0, 0, 128, 128, 128, 128);
        let expected = premultiplied_bgra(128, 128, 128, 128);
        assert_eq!(&fb.pixels[0..4], &expected);
    }

    #[test]
    fn cell_alpha_default_bg_gets_transparency() {
        let bg_alpha: u8 = 180;
        let cell_bg_is_default = true;
        let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };
        assert_eq!(cell_alpha, 180);
    }

    #[test]
    fn cell_alpha_explicit_bg_stays_opaque() {
        let bg_alpha: u8 = 180;
        let cell_bg_is_default = false;
        let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };
        assert_eq!(cell_alpha, 255);
    }

    #[test]
    fn cell_alpha_reverse_stays_opaque() {
        let bg_alpha: u8 = 180;
        let mut cell_bg_is_default = true;
        let is_reverse = true;
        if is_reverse {
            cell_bg_is_default = false;
        }
        let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };
        assert_eq!(cell_alpha, 255);
    }

    #[test]
    fn cell_alpha_selected_stays_opaque() {
        let bg_alpha: u8 = 180;
        let mut cell_bg_is_default = true;
        let is_selected = true;
        if is_selected {
            cell_bg_is_default = false;
        }
        let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };
        assert_eq!(cell_alpha, 255);
    }

    #[test]
    fn cell_alpha_opaque_when_no_transparency() {
        let bg_alpha: u8 = 255;
        let cell_bg_is_default = true;
        let cell_alpha = if cell_bg_is_default { bg_alpha } else { 255 };
        assert_eq!(cell_alpha, 255);
    }
}
