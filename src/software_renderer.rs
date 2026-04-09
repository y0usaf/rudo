//! Software framebuffer renderer scaffold for Wayland shm backend.
//! Pixels are BGRA8 little-endian.

use crate::cursor::CursorRenderer;
use crate::font::FontAtlas;
use crate::terminal::cell::{CellFlags, ColorSource, PackedColor};
use crate::terminal::grid::Grid;
use crate::terminal::selection::Selection;
use crate::terminal::theme::Theme;

/// Distance threshold for edge-strip rendering (cursor outline)
const EDGE_STRIP_DISTANCE_SQ: f32 = 0.8;
const FLOAT_CHANGE_EPSILON: f32 = 0.001;
const AXIS_ALIGNED_QUAD_EPSILON: f32 = 0.01;
const BT709_LUMA_RED: f32 = 0.2126;
const BT709_LUMA_GREEN: f32 = 0.7152;
const BT709_LUMA_BLUE: f32 = 0.0722;
const LUMA_CONTRAST_THRESHOLD: f32 = 0.5;
const MAX_COLOR_CHANNEL: f32 = 255.0;
const MAX_COLOR_CHANNEL_U8: u16 = 255;
const ALPHA_ROUND_BIAS_U8: u16 = 127;
const DEGENERATE_EDGE_EPSILON_SQ: f32 = 0.0001;
const DEFAULT_FONT_DPI: f32 = 96.0;
const POINTS_PER_INCH: f32 = 72.0;
pub(crate) const CURSOR_GEOMETRY_EPSILON: f32 = 0.001;

#[allow(dead_code)]
pub struct FrameBuffer<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: &'a mut [u8],
}

pub struct RenderState<'a> {
    pub grid: &'a mut Grid,
    pub cursor: &'a CursorRenderer,
    pub selection: &'a Selection,
    pub dirty_rows: &'a [(usize, usize)],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderOptions {
    pub full_redraw: bool,
    pub draw_cursor: bool,
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgb {
    red: u8,
    green: u8,
    blue: u8,
}

impl Rgb {
    const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    const fn from_packed(color: PackedColor) -> Self {
        Self::new(color.r(), color.g(), color.b())
    }
}

#[derive(Clone, Copy)]
struct GlyphStyle {
    color: Rgb,
    bold: bool,
    italic: bool,
}

impl GlyphStyle {
    const fn new(color: PackedColor, bold: bool, italic: bool) -> Self {
        Self {
            color: Rgb::from_packed(color),
            bold,
            italic,
        }
    }
}

#[allow(dead_code)]
pub struct SoftwareRenderer {
    font: FontAtlas,
    theme: Theme,
    font_family: String,
    font_size: f32,
    base_font_size: f32,
    scale: f32,
    cell_width: f32,
    cell_height: f32,
    baseline: f32,
    padding: u32,
    offset_x: f32,
    offset_y: f32,
    col_boundaries: Vec<u32>,
    row_boundaries: Vec<u32>,
}

#[allow(dead_code)]
impl SoftwareRenderer {
    pub fn new(font_size: f32, font_family: String, theme: Theme, padding: u32) -> Self {
        let font_size = font_size.max(1.0);
        let font = FontAtlas::new(Self::font_size_in_pixels(font_size, 1.0), &font_family);
        let cell_width = font.cell_width().max(1.0);
        let cell_height = font.cell_height().max(1.0);
        let baseline = font.baseline();

        Self {
            font,
            theme,
            font_family,
            font_size,
            base_font_size: font_size,
            scale: 1.0,
            cell_width,
            cell_height,
            baseline,
            padding,
            offset_x: 0.0,
            offset_y: 0.0,
            col_boundaries: Vec::new(),
            row_boundaries: Vec::new(),
        }
    }

    pub fn set_theme(&mut self, theme: &Theme) {
        self.theme = theme.clone();
    }

    pub fn set_scale(&mut self, scale: f32) {
        let scale = scale.max(1.0);
        if (self.scale - scale).abs() < FLOAT_CHANGE_EPSILON {
            return;
        }
        self.scale = scale;
        self.rebuild_font();
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
        (self.cell_width, self.cell_height)
    }

    fn pixel_boundary(origin: f32, step: f32, index: usize) -> u32 {
        (origin + step * index as f32).round().max(0.0) as u32
    }

    fn anchored_grid_layout(
        width: u32,
        height: u32,
        cell_width: f32,
        cell_height: f32,
        padding: u32,
        scale: f32,
    ) -> (usize, usize, f32, f32) {
        let cw = cell_width.max(1.0);
        let ch = cell_height.max(1.0);
        let phys_pad = padding as f32 * scale;
        let usable_w = (width as f32 - phys_pad * 2.0).max(0.0);
        let usable_h = (height as f32 - phys_pad * 2.0).max(0.0);
        let cols = (usable_w / cw).floor().max(1.0) as usize;
        let rows = (usable_h / ch).floor().max(1.0) as usize;

        // Match foot's default windowed layout more closely: do not center the
        // grid inside the surface. Centering creates unused slivers on all sides
        // whenever the window size is not an exact multiple of the cell size.
        // Instead, anchor the grid at the padded top-left origin and leave any
        // remainder on the right/bottom.
        let offset_x = phys_pad.min(width as f32);
        let offset_y = phys_pad.min(height as f32);
        (cols, rows, offset_x, offset_y)
    }

    fn col_boundary(&self, col: usize) -> u32 {
        self.col_boundaries
            .get(col)
            .copied()
            .unwrap_or_else(|| Self::pixel_boundary(self.offset_x, self.cell_width, col))
    }

    fn row_boundary(&self, row: usize) -> u32 {
        self.row_boundaries
            .get(row)
            .copied()
            .unwrap_or_else(|| Self::pixel_boundary(self.offset_y, self.cell_height, row))
    }

    pub fn pixel_bounds_for_row_range(
        &self,
        start_row: usize,
        end_row_inclusive: usize,
    ) -> (u32, u32) {
        let y0 = self.row_boundary(start_row);
        let y1 = self.row_boundary(end_row_inclusive.saturating_add(1));
        (y0, y1.max(y0))
    }

    pub fn window_size_for_grid(&self, cols: usize, rows: usize) -> (u32, u32) {
        let phys_pad = self.padding as f32 * self.scale;
        let width = Self::pixel_boundary(phys_pad, self.cell_width, cols)
            .saturating_add(phys_pad.round().max(0.0) as u32);
        let height = Self::pixel_boundary(phys_pad, self.cell_height, rows)
            .saturating_add(phys_pad.round().max(0.0) as u32);
        (width.max(1), height.max(1))
    }

    /// Compute grid dimensions from physical (scaled) pixel dimensions.
    pub fn grid_size_for_window(&mut self, width: u32, height: u32) -> (usize, usize) {
        let (cols, rows, offset_x, offset_y) = Self::anchored_grid_layout(
            width,
            height,
            self.cell_width,
            self.cell_height,
            self.padding,
            self.scale,
        );
        self.offset_x = offset_x;
        self.offset_y = offset_y;
        self.update_grid_boundaries(cols, rows);
        (cols, rows)
    }

    /// Returns the pixel offset from the top-left of the window to the
    /// top-left of the first grid cell.
    pub fn grid_offset(&self) -> (f32, f32) {
        (self.offset_x, self.offset_y)
    }

    fn update_grid_boundaries(&mut self, cols: usize, rows: usize) {
        self.col_boundaries.clear();
        self.col_boundaries.reserve(cols + 1);
        for col in 0..=cols {
            self.col_boundaries
                .push(Self::pixel_boundary(self.offset_x, self.cell_width, col));
        }

        self.row_boundaries.clear();
        self.row_boundaries.reserve(rows + 1);
        for row in 0..=rows {
            self.row_boundaries
                .push(Self::pixel_boundary(self.offset_y, self.cell_height, row));
        }
    }

    fn font_size_in_pixels(font_size: f32, scale: f32) -> f32 {
        // Match foot/fontconfig semantics: configured sizes are point sizes,
        // which at the default 96 DPI become pt * 96 / 72 device pixels.
        (font_size.max(1.0) * scale.max(1.0) * DEFAULT_FONT_DPI / POINTS_PER_INCH).max(1.0)
    }

    fn rebuild_font(&mut self) {
        let physical_size = Self::font_size_in_pixels(self.font_size, self.scale);
        let font = FontAtlas::new(physical_size, &self.font_family);
        self.cell_width = font.cell_width().max(1.0);
        self.cell_height = font.cell_height().max(1.0);
        self.baseline = font.baseline();
        self.font = font;
    }

    pub fn render(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        state: RenderState<'_>,
        options: RenderOptions,
    ) {
        let RenderState {
            grid,
            cursor,
            selection,
            dirty_rows,
        } = state;
        let RenderOptions {
            full_redraw,
            draw_cursor,
        } = options;
        if self.col_boundaries.len() != grid.cols().saturating_add(1)
            || self.row_boundaries.len() != grid.rows().saturating_add(1)
        {
            self.update_grid_boundaries(grid.cols(), grid.rows());
        }

        let background = self.theme.background;
        let background_rgb = Rgb::from_packed(background);
        let selection_bg = self.theme.selection;
        let normalized_selection = selection.has_selection().then(|| selection.normalized());
        let top_pad = self
            .row_boundaries
            .first()
            .copied()
            .unwrap_or_default()
            .min(fb.height);

        if full_redraw && top_pad > 0 {
            self.fill_rect(fb, Rect::new(0, 0, fb.width, top_pad), background_rgb);
        }

        let grid_rows = grid.rows();
        let mut render_row = |row: usize| {
            let (selected_start, selected_end) = normalized_selection
                .and_then(|(start, end)| {
                    if row < start.row || row > end.row {
                        None
                    } else if start.row == end.row {
                        Some((start.col, end.col))
                    } else if row == start.row {
                        Some((start.col, usize::MAX))
                    } else if row == end.row {
                        Some((0, end.col))
                    } else {
                        Some((0, usize::MAX))
                    }
                })
                .unwrap_or((usize::MAX, usize::MIN));
            let y0 = self.row_boundaries[row].min(fb.height);
            let y1 = self.row_boundaries[row + 1].min(fb.height);
            let cell_h = y1.saturating_sub(y0);

            self.fill_rect(fb, Rect::new(0, y0, fb.width, cell_h), background_rgb);

            let row_state = grid.view_row_mut(row);
            {
                let row_cells = row_state.cells();
                for (col, cell) in row_cells.iter().copied().enumerate() {
                    let mut fg = if cell.fg_src == ColorSource::Default {
                        self.theme.foreground
                    } else {
                        cell.fg
                    };
                    let mut bg = if cell.bg_src == ColorSource::Default {
                        background
                    } else {
                        cell.bg
                    };

                    if cell.flags.contains(CellFlags::REVERSE) {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if col >= selected_start && col <= selected_end {
                        bg = selection_bg;
                    }

                    let x0 = self.col_boundaries[col].min(fb.width);
                    let x1 = self.col_boundaries[col + 1].min(fb.width);
                    let cell_w = x1.saturating_sub(x0);
                    if bg != background {
                        self.fill_rect(fb, Rect::new(x0, y0, cell_w, cell_h), Rgb::from_packed(bg));
                    }

                    let ch = cell.character();
                    if ch != ' '
                        && !cell.flags.contains(CellFlags::HIDDEN)
                        && !cell.flags.contains(CellFlags::WIDE_SPACER)
                    {
                        self.draw_cell_glyph(
                            fb,
                            x0,
                            y0,
                            cell_w,
                            cell_h,
                            ch,
                            GlyphStyle::new(
                                fg,
                                cell.flags.contains(CellFlags::BOLD),
                                cell.flags.contains(CellFlags::ITALIC),
                            ),
                        );
                    }
                }
            }

            row_state.clear_dirty();
        };

        if full_redraw {
            for row in 0..grid_rows {
                render_row(row);
            }
        } else {
            let last_row = grid_rows.saturating_sub(1);
            for &(start_row, end_row) in dirty_rows {
                for row in start_row..=end_row.min(last_row) {
                    render_row(row);
                }
            }
        }

        let grid_bottom = self.row_boundaries[grid.rows()].min(fb.height);
        if full_redraw && grid_bottom < fb.height {
            self.fill_rect(
                fb,
                Rect::new(0, grid_bottom, fb.width, fb.height - grid_bottom),
                background_rgb,
            );
        }

        if draw_cursor
            && grid.cursor_visible()
            && cursor.is_visible()
            && !grid.is_viewing_scrollback()
        {
            self.draw_animated_cursor(fb, grid, cursor);
        }
    }

    fn fill_rect(&self, fb: &mut FrameBuffer<'_>, rect: Rect, color: Rgb) {
        let max_x = rect.x.saturating_add(rect.width).min(fb.width);
        let max_y = rect.y.saturating_add(rect.height).min(fb.height);
        if rect.x >= max_x || rect.y >= max_y {
            return;
        }

        fill_rect_raw(fb, rect, color);
    }

    fn draw_animated_cursor(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        grid: &Grid,
        cursor: &CursorRenderer,
    ) {
        let corners_grid = cursor.corner_positions();
        let ox = self.offset_x;
        let oy = self.offset_y;
        let corners_px = [
            (
                ox + corners_grid[0].0 * self.cell_width,
                oy + corners_grid[0].1 * self.cell_height,
            ),
            (
                ox + corners_grid[1].0 * self.cell_width,
                oy + corners_grid[1].1 * self.cell_height,
            ),
            (
                ox + corners_grid[2].0 * self.cell_width,
                oy + corners_grid[2].1 * self.cell_height,
            ),
            (
                ox + corners_grid[3].0 * self.cell_width,
                oy + corners_grid[3].1 * self.cell_height,
            ),
        ];

        let cursor_color = Rgb::from_packed(self.theme.cursor);
        match cursor.shape {
            crate::cursor::CursorShape::Block => {
                self.fill_quad(fb, corners_px, cursor_color);
                let cursor_col = grid.cursor_col().min(grid.cols().saturating_sub(1));
                let cursor_row = grid.cursor_row().min(grid.rows().saturating_sub(1));
                let cell = grid.cell(cursor_col, cursor_row);
                let cell_x = self.col_boundary(cursor_col);
                let cell_y = self.row_boundary(cursor_row);
                let cell_w = self
                    .col_boundary(cursor_col.saturating_add(1))
                    .saturating_sub(cell_x);
                let cell_h = self
                    .row_boundary(cursor_row.saturating_add(1))
                    .saturating_sub(cell_y);
                self.draw_cell_glyph_clipped(
                    fb,
                    cell_x,
                    cell_y,
                    cell_w,
                    cell_h,
                    cell.character(),
                    GlyphStyle {
                        color: contrasting_cursor_text_color(cursor_color),
                        bold: cell.flags.contains(CellFlags::BOLD),
                        italic: cell.flags.contains(CellFlags::ITALIC),
                    },
                    corners_px,
                );
            }
            crate::cursor::CursorShape::Beam | crate::cursor::CursorShape::Underline => {
                self.stroke_quad_edges(fb, corners_px, cursor_color);
            }
        }
    }

    fn fill_quad(&self, fb: &mut FrameBuffer<'_>, quad: [(f32, f32); 4], color: Rgb) {
        if let Some((x, y, width, height)) =
            axis_aligned_quad_fill_bounds(quad, fb.width, fb.height)
        {
            fill_rect_raw(fb, Rect::new(x, y, width, height), color);
            return;
        }

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
                    put_bgra(fb, x, y, color, 255);
                }
            }
        }
    }

    fn stroke_quad_edges(&self, fb: &mut FrameBuffer<'_>, quad: [(f32, f32); 4], color: Rgb) {
        let edges = [
            (quad[0], quad[1]),
            (quad[1], quad[2]),
            (quad[2], quad[3]),
            (quad[3], quad[0]),
        ];
        for &(a, c) in &edges {
            self.fill_edge_strip(fb, a, c, color);
        }
    }

    fn fill_edge_strip(
        &self,
        fb: &mut FrameBuffer<'_>,
        start: (f32, f32),
        end: (f32, f32),
        color: Rgb,
    ) {
        let min_x = start.0.min(end.0).floor().max(0.0) as i32 - 1;
        let max_x = start.0.max(end.0).ceil().min(fb.width as f32) as i32 + 1;
        let min_y = start.1.min(end.1).floor().max(0.0) as i32 - 1;
        let max_y = start.1.max(end.1).ceil().min(fb.height as f32) as i32 + 1;
        let delta_x = end.0 - start.0;
        let delta_y = end.1 - start.1;
        let length_sq = delta_x * delta_x + delta_y * delta_y;
        if length_sq <= DEGENERATE_EDGE_EPSILON_SQ {
            return;
        }
        for y in min_y.max(0) as u32..(max_y.max(0) as u32).min(fb.height) {
            for x in min_x.max(0) as u32..(max_x.max(0) as u32).min(fb.width) {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let from_start_x = px - start.0;
                let from_start_y = py - start.1;
                let along =
                    ((from_start_x * delta_x + from_start_y * delta_y) / length_sq).clamp(0.0, 1.0);
                let closest_x = start.0 + along * delta_x;
                let closest_y = start.1 + along * delta_y;
                let dist_x = px - closest_x;
                let dist_y = py - closest_y;
                if dist_x * dist_x + dist_y * dist_y <= EDGE_STRIP_DISTANCE_SQ {
                    put_bgra(fb, x, y, color, 255);
                }
            }
        }
    }

    fn draw_cell_glyph_clipped(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        cell_x: u32,
        cell_y: u32,
        cell_w: u32,
        cell_h: u32,
        ch: char,
        style: GlyphStyle,
        clip_quad: [(f32, f32); 4],
    ) {
        self.draw_glyph_impl(
            fb,
            cell_x,
            cell_y,
            cell_w,
            cell_h,
            ch,
            style,
            Some(clip_quad),
        );
    }

    fn draw_cell_glyph(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        cell_x: u32,
        cell_y: u32,
        cell_w: u32,
        cell_h: u32,
        ch: char,
        style: GlyphStyle,
    ) {
        self.draw_glyph_impl(fb, cell_x, cell_y, cell_w, cell_h, ch, style, None);
    }

    fn draw_glyph_impl(
        &mut self,
        fb: &mut FrameBuffer<'_>,
        cell_x: u32,
        cell_y: u32,
        cell_w: u32,
        cell_h: u32,
        ch: char,
        style: GlyphStyle,
        clip_quad: Option<[(f32, f32); 4]>,
    ) {
        if clip_quad.is_none() {
            let cell_rect = Rect::new(cell_x, cell_y, cell_w, cell_h);
            if draw_native_box_char(fb, cell_rect, ch, style.color)
                || draw_native_block_char(fb, cell_rect, ch, style.color)
            {
                return;
            }
        }
        let glyph = self.font.get_glyph(ch, style.bold, style.italic);
        if glyph.width <= 0.0 || glyph.height <= 0.0 {
            return;
        }

        let (atlas_w, atlas_h) = self.font.atlas_size();
        let atlas = self.font.atlas_data();
        let src_left = (glyph.u0 * atlas_w as f32).round() as u32;
        let src_top = (glyph.v0 * atlas_h as f32).round() as u32;
        let glyph_width = glyph.width as u32;
        let glyph_height = glyph.height as u32;
        let dst_left = (cell_x as f32 + glyph.offset_x).round() as i32;
        let dst_top =
            (cell_y as f32 + self.baseline - glyph.height - glyph.offset_y).round() as i32;

        let src_width = glyph_width.min(atlas_w.saturating_sub(src_left));
        let src_height = glyph_height.min(atlas_h.saturating_sub(src_top));
        if src_width == 0 || src_height == 0 {
            return;
        }

        let skip_left = dst_left.saturating_neg().max(0) as u32;
        let skip_top = dst_top.saturating_neg().max(0) as u32;
        let skip_right = (dst_left + src_width as i32 - fb.width as i32).max(0) as u32;
        let skip_bottom = (dst_top + src_height as i32 - fb.height as i32).max(0) as u32;
        let draw_w = src_width
            .saturating_sub(skip_left)
            .saturating_sub(skip_right);
        let draw_h = src_height
            .saturating_sub(skip_top)
            .saturating_sub(skip_bottom);
        if draw_w == 0 || draw_h == 0 {
            return;
        }

        let src_x = src_left + skip_left;
        let src_y = src_top + skip_top;
        let dst_x = (dst_left + skip_left as i32) as u32;
        let dst_y = (dst_top + skip_top as i32) as u32;
        let src_row_stride = atlas_w as usize;

        for row in 0..draw_h {
            let src_row = (src_y + row) as usize * src_row_stride + src_x as usize;
            let dst_row = dst_y + row;
            for col in 0..draw_w {
                let alpha = atlas[src_row + col as usize];
                if alpha == 0 {
                    continue;
                }
                let dst_col = dst_x + col;
                if let Some(quad) = clip_quad {
                    let p = (dst_col as f32 + 0.5, dst_row as f32 + 0.5);
                    if !point_in_triangle(p, quad[0], quad[1], quad[2])
                        && !point_in_triangle(p, quad[0], quad[2], quad[3])
                    {
                        continue;
                    }
                }
                blend_bgra(fb, dst_col, dst_row, style.color, alpha);
            }
        }
    }
}

fn axis_aligned_quad_fill_bounds(
    quad: [(f32, f32); 4],
    fb_width: u32,
    fb_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let min_x = quad.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let min_y = quad.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let max_x = quad.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let max_y = quad.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    let expected = [
        (min_x, min_y),
        (max_x, min_y),
        (max_x, max_y),
        (min_x, max_y),
    ];

    if quad.iter().zip(expected).any(|(actual, wanted)| {
        (actual.0 - wanted.0).abs() > AXIS_ALIGNED_QUAD_EPSILON
            || (actual.1 - wanted.1).abs() > AXIS_ALIGNED_QUAD_EPSILON
    }) {
        return None;
    }

    let (x0, x1) = axis_aligned_fill_span(min_x, max_x, fb_width)?;
    let (y0, y1) = axis_aligned_fill_span(min_y, max_y, fb_height)?;
    Some((x0, y0, x1 - x0, y1 - y0))
}

fn axis_aligned_fill_span(start: f32, end: f32, limit: u32) -> Option<(u32, u32)> {
    let pixel_start = (start - 0.5).ceil().max(0.0).min(limit as f32) as u32;
    let pixel_end = ((end - 0.5).floor() + 1.0).max(0.0).min(limit as f32) as u32;
    (pixel_end > pixel_start).then_some((pixel_start, pixel_end))
}

fn point_in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s1 = (p.0 - c.0) * (a.1 - c.1) - (a.0 - c.0) * (p.1 - c.1);
    let s2 = (p.0 - a.0) * (b.1 - a.1) - (b.0 - a.0) * (p.1 - a.1);
    let s3 = (p.0 - b.0) * (c.1 - b.1) - (c.0 - b.0) * (p.1 - b.1);
    let has_neg = s1 < 0.0 || s2 < 0.0 || s3 < 0.0;
    let has_pos = s1 > 0.0 || s2 > 0.0 || s3 > 0.0;
    !(has_neg && has_pos)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrokeWeight {
    Light,
    Heavy,
    Double,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BoxDrawingSpec {
    left: Option<StrokeWeight>,
    right: Option<StrokeWeight>,
    up: Option<StrokeWeight>,
    down: Option<StrokeWeight>,
}

#[derive(Clone, Copy, Debug)]
struct AxisStrokeLayout {
    bands: [(u32, u32); 2],
    band_count: usize,
    join_start: u32,
    join_end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HorizontalSegment {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerticalSegment {
    Up,
    Down,
}

fn draw_native_box_char(fb: &mut FrameBuffer<'_>, rect: Rect, ch: char, color: Rgb) -> bool {
    let Some(spec) = box_drawing_spec(ch) else {
        return false;
    };
    if rect.width == 0 || rect.height == 0 {
        return true;
    }

    let light = light_box_stroke_thickness(rect);
    let heavy = heavy_box_stroke_thickness(rect, light);

    if let Some(weight) = spec.left {
        draw_box_horizontal_segment(
            fb,
            rect,
            weight,
            HorizontalSegment::Left,
            light,
            heavy,
            color,
        );
    }
    if let Some(weight) = spec.right {
        draw_box_horizontal_segment(
            fb,
            rect,
            weight,
            HorizontalSegment::Right,
            light,
            heavy,
            color,
        );
    }
    if let Some(weight) = spec.up {
        draw_box_vertical_segment(fb, rect, weight, VerticalSegment::Up, light, heavy, color);
    }
    if let Some(weight) = spec.down {
        draw_box_vertical_segment(fb, rect, weight, VerticalSegment::Down, light, heavy, color);
    }

    true
}

fn draw_native_block_char(fb: &mut FrameBuffer<'_>, rect: Rect, ch: char, color: Rgb) -> bool {
    match ch {
        '█' => fill_rect_raw(fb, rect, color),
        '▀' => fill_fraction_rect(fb, rect, 0, 8, 0, 4, color),
        '▄' => fill_fraction_rect(fb, rect, 0, 8, 4, 8, color),
        '▁' => fill_fraction_rect(fb, rect, 0, 8, 7, 8, color),
        '▂' => fill_fraction_rect(fb, rect, 0, 8, 6, 8, color),
        '▃' => fill_fraction_rect(fb, rect, 0, 8, 5, 8, color),
        '▅' => fill_fraction_rect(fb, rect, 0, 8, 3, 8, color),
        '▆' => fill_fraction_rect(fb, rect, 0, 8, 2, 8, color),
        '▇' => fill_fraction_rect(fb, rect, 0, 8, 1, 8, color),
        '▉' => fill_fraction_rect(fb, rect, 0, 7, 0, 8, color),
        '▊' => fill_fraction_rect(fb, rect, 0, 6, 0, 8, color),
        '▋' => fill_fraction_rect(fb, rect, 0, 5, 0, 8, color),
        '▌' => fill_fraction_rect(fb, rect, 0, 4, 0, 8, color),
        '▍' => fill_fraction_rect(fb, rect, 0, 3, 0, 8, color),
        '▎' => fill_fraction_rect(fb, rect, 0, 2, 0, 8, color),
        '▏' => fill_fraction_rect(fb, rect, 0, 1, 0, 8, color),
        '▐' => fill_fraction_rect(fb, rect, 4, 8, 0, 8, color),
        '▔' => fill_fraction_rect(fb, rect, 0, 8, 0, 1, color),
        '▕' => fill_fraction_rect(fb, rect, 7, 8, 0, 8, color),
        '▖' => fill_fraction_rect(fb, rect, 0, 4, 4, 8, color),
        '▗' => fill_fraction_rect(fb, rect, 4, 8, 4, 8, color),
        '▘' => fill_fraction_rect(fb, rect, 0, 4, 0, 4, color),
        '▙' => {
            fill_fraction_rect(fb, rect, 0, 4, 0, 8, color);
            fill_fraction_rect(fb, rect, 4, 8, 4, 8, color);
        }
        '▚' => {
            fill_fraction_rect(fb, rect, 0, 4, 0, 4, color);
            fill_fraction_rect(fb, rect, 4, 8, 4, 8, color);
        }
        '▛' => {
            fill_fraction_rect(fb, rect, 0, 8, 0, 4, color);
            fill_fraction_rect(fb, rect, 0, 4, 4, 8, color);
        }
        '▜' => {
            fill_fraction_rect(fb, rect, 0, 8, 0, 4, color);
            fill_fraction_rect(fb, rect, 4, 8, 4, 8, color);
        }
        '▝' => fill_fraction_rect(fb, rect, 4, 8, 0, 4, color),
        '▞' => {
            fill_fraction_rect(fb, rect, 4, 8, 0, 4, color);
            fill_fraction_rect(fb, rect, 0, 4, 4, 8, color);
        }
        '▟' => {
            fill_fraction_rect(fb, rect, 0, 8, 4, 8, color);
            fill_fraction_rect(fb, rect, 4, 8, 0, 4, color);
        }
        _ => return false,
    }

    true
}

fn fill_fraction_rect(
    fb: &mut FrameBuffer<'_>,
    rect: Rect,
    x0_num: u32,
    x1_num: u32,
    y0_num: u32,
    y1_num: u32,
    color: Rgb,
) {
    let x0 = rect.x + rect.width.saturating_mul(x0_num) / 8;
    let x1 = rect.x + rect.width.saturating_mul(x1_num) / 8;
    let y0 = rect.y + rect.height.saturating_mul(y0_num) / 8;
    let y1 = rect.y + rect.height.saturating_mul(y1_num) / 8;
    if x1 > x0 && y1 > y0 {
        fill_rect_raw(fb, Rect::new(x0, y0, x1 - x0, y1 - y0), color);
    }
}

fn light_box_stroke_thickness(rect: Rect) -> u32 {
    (rect.width.min(rect.height) / 10).clamp(1, 2)
}

fn heavy_box_stroke_thickness(rect: Rect, light: u32) -> u32 {
    if rect.width.min(rect.height) <= 2 {
        1
    } else {
        (light + 1).clamp(2, 3)
    }
}

fn draw_box_horizontal_segment(
    fb: &mut FrameBuffer<'_>,
    rect: Rect,
    weight: StrokeWeight,
    segment: HorizontalSegment,
    light: u32,
    heavy: u32,
    color: Rgb,
) {
    let y_layout = axis_stroke_layout(rect.y, rect.height, weight, light, heavy);
    let x_layout = axis_stroke_layout(rect.x, rect.width, weight, light, heavy);
    let (x0, x1) = match segment {
        HorizontalSegment::Left => (rect.x, x_layout.join_end),
        HorizontalSegment::Right => (x_layout.join_start, rect.x.saturating_add(rect.width)),
    };
    if x1 <= x0 {
        return;
    }

    for &(y0, y1) in y_layout.bands.iter().take(y_layout.band_count) {
        if y1 > y0 {
            fill_rect_raw(fb, Rect::new(x0, y0, x1 - x0, y1 - y0), color);
        }
    }
}

fn draw_box_vertical_segment(
    fb: &mut FrameBuffer<'_>,
    rect: Rect,
    weight: StrokeWeight,
    segment: VerticalSegment,
    light: u32,
    heavy: u32,
    color: Rgb,
) {
    let x_layout = axis_stroke_layout(rect.x, rect.width, weight, light, heavy);
    let y_layout = axis_stroke_layout(rect.y, rect.height, weight, light, heavy);
    let (y0, y1) = match segment {
        VerticalSegment::Up => (rect.y, y_layout.join_end),
        VerticalSegment::Down => (y_layout.join_start, rect.y.saturating_add(rect.height)),
    };
    if y1 <= y0 {
        return;
    }

    for &(x0, x1) in x_layout.bands.iter().take(x_layout.band_count) {
        if x1 > x0 {
            fill_rect_raw(fb, Rect::new(x0, y0, x1 - x0, y1 - y0), color);
        }
    }
}

fn axis_stroke_layout(
    origin: u32,
    extent: u32,
    weight: StrokeWeight,
    light: u32,
    heavy: u32,
) -> AxisStrokeLayout {
    let extent = extent.max(1);

    match weight {
        StrokeWeight::Light | StrokeWeight::Heavy => {
            let thickness = match weight {
                StrokeWeight::Light => light,
                StrokeWeight::Heavy => heavy,
                StrokeWeight::Double => unreachable!(),
            }
            .clamp(1, extent);
            let (start, end) = centered_span(origin, extent, thickness);
            AxisStrokeLayout {
                bands: [(start, end), (0, 0)],
                band_count: 1,
                join_start: start,
                join_end: end,
            }
        }
        StrokeWeight::Double => {
            let thickness = light.clamp(1, extent);
            let max_gap = extent.saturating_sub(thickness.saturating_mul(2));
            if max_gap == 0 {
                let (start, end) = centered_span(origin, extent, thickness);
                return AxisStrokeLayout {
                    bands: [(start, end), (0, 0)],
                    band_count: 1,
                    join_start: start,
                    join_end: end,
                };
            }

            let gap = max_gap.min(thickness.max(1));
            let total = thickness.saturating_mul(2).saturating_add(gap);
            let (join_start, join_end) = centered_span(origin, extent, total);
            let first = (join_start, join_start + thickness);
            let second_start = join_end.saturating_sub(thickness);
            let second = (second_start, second_start + thickness);
            AxisStrokeLayout {
                bands: [first, second],
                band_count: 2,
                join_start,
                join_end,
            }
        }
    }
}

fn centered_span(origin: u32, extent: u32, thickness: u32) -> (u32, u32) {
    let thickness = thickness.clamp(1, extent.max(1));
    let start = origin + extent.saturating_sub(thickness) / 2;
    (start, start + thickness)
}

fn box_drawing_spec(ch: char) -> Option<BoxDrawingSpec> {
    use StrokeWeight::{Double as D, Heavy as H, Light as L};

    Some(match ch {
        '─' => BoxDrawingSpec {
            left: Some(L),
            right: Some(L),
            ..BoxDrawingSpec::default()
        },
        '━' => BoxDrawingSpec {
            left: Some(H),
            right: Some(H),
            ..BoxDrawingSpec::default()
        },
        '│' => BoxDrawingSpec {
            up: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┃' => BoxDrawingSpec {
            up: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┌' | '╭' => BoxDrawingSpec {
            right: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┐' | '╮' => BoxDrawingSpec {
            left: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '└' | '╰' => BoxDrawingSpec {
            right: Some(L),
            up: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┘' | '╯' => BoxDrawingSpec {
            left: Some(L),
            up: Some(L),
            ..BoxDrawingSpec::default()
        },
        '├' => BoxDrawingSpec {
            right: Some(L),
            up: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┤' => BoxDrawingSpec {
            left: Some(L),
            up: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┬' => BoxDrawingSpec {
            left: Some(L),
            right: Some(L),
            down: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┴' => BoxDrawingSpec {
            left: Some(L),
            right: Some(L),
            up: Some(L),
            ..BoxDrawingSpec::default()
        },
        '┼' => BoxDrawingSpec {
            left: Some(L),
            right: Some(L),
            up: Some(L),
            down: Some(L),
        },
        '┏' => BoxDrawingSpec {
            right: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┓' => BoxDrawingSpec {
            left: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┗' => BoxDrawingSpec {
            right: Some(H),
            up: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┛' => BoxDrawingSpec {
            left: Some(H),
            up: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┣' => BoxDrawingSpec {
            right: Some(H),
            up: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┫' => BoxDrawingSpec {
            left: Some(H),
            up: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┳' => BoxDrawingSpec {
            left: Some(H),
            right: Some(H),
            down: Some(H),
            ..BoxDrawingSpec::default()
        },
        '┻' => BoxDrawingSpec {
            left: Some(H),
            right: Some(H),
            up: Some(H),
            ..BoxDrawingSpec::default()
        },
        '╋' => BoxDrawingSpec {
            left: Some(H),
            right: Some(H),
            up: Some(H),
            down: Some(H),
        },
        '═' => BoxDrawingSpec {
            left: Some(D),
            right: Some(D),
            ..BoxDrawingSpec::default()
        },
        '║' => BoxDrawingSpec {
            up: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╔' => BoxDrawingSpec {
            right: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╗' => BoxDrawingSpec {
            left: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╚' => BoxDrawingSpec {
            right: Some(D),
            up: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╝' => BoxDrawingSpec {
            left: Some(D),
            up: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╠' => BoxDrawingSpec {
            right: Some(D),
            up: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╣' => BoxDrawingSpec {
            left: Some(D),
            up: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╦' => BoxDrawingSpec {
            left: Some(D),
            right: Some(D),
            down: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╩' => BoxDrawingSpec {
            left: Some(D),
            right: Some(D),
            up: Some(D),
            ..BoxDrawingSpec::default()
        },
        '╬' => BoxDrawingSpec {
            left: Some(D),
            right: Some(D),
            up: Some(D),
            down: Some(D),
        },
        _ => return None,
    })
}

fn contrasting_cursor_text_color(color: Rgb) -> Rgb {
    let luma = (BT709_LUMA_RED * color.red as f32
        + BT709_LUMA_GREEN * color.green as f32
        + BT709_LUMA_BLUE * color.blue as f32)
        / MAX_COLOR_CHANNEL;
    if luma > LUMA_CONTRAST_THRESHOLD {
        Rgb::new(0, 0, 0)
    } else {
        Rgb::new(255, 255, 255)
    }
}

/// 8-bit premultiply — used only for glyph texel compositing where the
/// source alpha is already 8-bit from the font atlas.
fn premultiply_8(channel: u8, alpha: u8) -> u8 {
    ((channel as u16 * alpha as u16 + ALPHA_ROUND_BIAS_U8) / MAX_COLOR_CHANNEL_U8) as u8
}

fn premultiplied_bgra_8(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [
        premultiply_8(b, a),
        premultiply_8(g, a),
        premultiply_8(r, a),
        a,
    ]
}

fn fill_rect_raw(fb: &mut FrameBuffer<'_>, rect: Rect, color: Rgb) {
    let max_x = rect.x.saturating_add(rect.width).min(fb.width);
    let max_y = rect.y.saturating_add(rect.height).min(fb.height);
    if rect.x >= max_x || rect.y >= max_y {
        return;
    }

    let pixel = premultiplied_bgra_8(color.red, color.green, color.blue, 255);
    let start_x = rect.x as usize * 4;
    let row_bytes = (max_x - rect.x) as usize * 4;
    let stride = fb.stride as usize;

    for py in rect.y as usize..max_y as usize {
        let row_start = py * stride + start_x;
        let row_end = row_start.saturating_add(row_bytes).min(fb.pixels.len());
        let row = &mut fb.pixels[row_start..row_end];
        if row.len() < 4 {
            continue;
        }

        row[..4].copy_from_slice(&pixel);
        let mut filled = 4;
        while filled < row.len() {
            let copy_len = (row.len() - filled).min(filled);
            let (left, right) = row.split_at_mut(filled);
            right[..copy_len].copy_from_slice(&left[..copy_len]);
            filled += copy_len;
        }
    }
}

fn put_bgra(fb: &mut FrameBuffer<'_>, x: u32, y: u32, color: Rgb, alpha: u8) {
    let idx = y as usize * fb.stride as usize + x as usize * 4;
    if idx + 3 >= fb.pixels.len() {
        return;
    }
    let pixel = premultiplied_bgra_8(color.red, color.green, color.blue, alpha);
    fb.pixels[idx] = pixel[0];
    fb.pixels[idx + 1] = pixel[1];
    fb.pixels[idx + 2] = pixel[2];
    fb.pixels[idx + 3] = pixel[3];
}

fn blend_bgra(fb: &mut FrameBuffer<'_>, x: u32, y: u32, color: Rgb, alpha: u8) {
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        put_bgra(fb, x, y, color, alpha);
        return;
    }

    let idx = y as usize * fb.stride as usize + x as usize * 4;
    if idx + 3 >= fb.pixels.len() {
        return;
    }

    let src = premultiplied_bgra_8(color.red, color.green, color.blue, alpha);
    let dst_b = fb.pixels[idx] as u16;
    let dst_g = fb.pixels[idx + 1] as u16;
    let dst_r = fb.pixels[idx + 2] as u16;
    let dst_a = fb.pixels[idx + 3] as u16;
    let src_a = src[3] as u16;
    let inv = MAX_COLOR_CHANNEL_U8 - src_a;

    fb.pixels[idx] = (src[0] as u16 + ((dst_b * inv + ALPHA_ROUND_BIAS_U8) / MAX_COLOR_CHANNEL_U8))
        .min(MAX_COLOR_CHANNEL_U8) as u8;
    fb.pixels[idx + 1] = (src[1] as u16
        + ((dst_g * inv + ALPHA_ROUND_BIAS_U8) / MAX_COLOR_CHANNEL_U8))
        .min(MAX_COLOR_CHANNEL_U8) as u8;
    fb.pixels[idx + 2] = (src[2] as u16
        + ((dst_r * inv + ALPHA_ROUND_BIAS_U8) / MAX_COLOR_CHANNEL_U8))
        .min(MAX_COLOR_CHANNEL_U8) as u8;
    fb.pixels[idx + 3] = (src_a + ((dst_a * inv + ALPHA_ROUND_BIAS_U8) / MAX_COLOR_CHANNEL_U8))
        .min(MAX_COLOR_CHANNEL_U8) as u8;
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::damage::DamageTracker;

    fn make_fb(w: u32, h: u32) -> (Vec<u8>, u32) {
        let stride = w * 4;
        let pixels = vec![0u8; (stride * h) as usize];
        (pixels, stride)
    }

    #[test]
    fn premultiply_8_opaque() {
        assert_eq!(premultiply_8(255, 255), 255);
        assert_eq!(premultiply_8(128, 255), 128);
        assert_eq!(premultiply_8(0, 255), 0);
    }

    #[test]
    fn premultiply_8_transparent() {
        assert_eq!(premultiply_8(255, 0), 0);
        assert_eq!(premultiply_8(128, 0), 0);
        assert_eq!(premultiply_8(0, 0), 0);
    }

    #[test]
    fn premultiply_8_half() {
        assert_eq!(premultiply_8(255, 128), 128);
        assert_eq!(premultiply_8(128, 128), 64);
    }

    #[test]
    fn premultiplied_bgra_8_opaque() {
        let px = premultiplied_bgra_8(255, 128, 64, 255);
        assert_eq!(px, [64, 128, 255, 255]);
    }

    #[test]
    fn premultiplied_bgra_8_transparent() {
        let px = premultiplied_bgra_8(200, 100, 50, 0);
        assert_eq!(px, [0, 0, 0, 0]);
    }

    #[test]
    fn premultiplied_bgra_8_half_alpha() {
        let px = premultiplied_bgra_8(255, 255, 255, 128);
        assert_eq!(px, [128, 128, 128, 128]);
    }

    #[test]
    fn pixel_byte_order_matches_argb8888_le() {
        let px = premultiplied_bgra_8(0xFF, 0x00, 0x80, 0xFF);
        assert_eq!(px, [0x80, 0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn grid_is_anchored_top_left_not_centered() {
        let (cols, rows, offset_x, offset_y) =
            SoftwareRenderer::anchored_grid_layout(803, 607, 9.0, 18.0, 0, 1.0);
        assert!(cols >= 1 && rows >= 1);
        assert_eq!((offset_x, offset_y), (0.0, 0.0));
    }

    #[test]
    fn fractional_cell_metrics_preserve_expected_column_count() {
        let (cols, rows, offset_x, offset_y) =
            SoftwareRenderer::anchored_grid_layout(104, 208, 10.4, 20.8, 0, 1.0);
        assert_eq!(cols, 10);
        assert_eq!(rows, 10);
        assert_eq!((offset_x, offset_y), (0.0, 0.0));
    }

    #[test]
    fn font_sizes_are_interpreted_as_points() {
        assert!((SoftwareRenderer::font_size_in_pixels(16.0, 1.0) - 21.333334).abs() < 0.001);
        assert!((SoftwareRenderer::font_size_in_pixels(16.0, 1.5) - 32.0).abs() < 0.001);
    }

    #[test]
    fn blend_full_coverage_replaces_dst() {
        let (mut px, stride) = make_fb(1, 1);
        px[0..4].copy_from_slice(&premultiplied_bgra_8(255, 255, 255, 128));
        let mut fb = FrameBuffer {
            width: 1,
            height: 1,
            stride,
            pixels: &mut px,
        };
        blend_bgra(&mut fb, 0, 0, Rgb::new(0, 0, 0), 255);
        assert_eq!(&fb.pixels[0..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn blend_zero_coverage_preserves_dst() {
        let (mut px, stride) = make_fb(1, 1);
        px[0..4].copy_from_slice(&premultiplied_bgra_8(100, 200, 50, 180));
        let saved = [px[0], px[1], px[2], px[3]];
        let mut fb = FrameBuffer {
            width: 1,
            height: 1,
            stride,
            pixels: &mut px,
        };
        blend_bgra(&mut fb, 0, 0, Rgb::new(255, 255, 255), 0);
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
        blend_bgra(&mut fb, 0, 0, Rgb::new(128, 128, 128), 128);
        let expected = premultiplied_bgra_8(128, 128, 128, 128);
        assert_eq!(&fb.pixels[0..4], &expected);
    }

    #[test]
    fn fill_rect_writes_opaque_pixels() {
        let (mut px, stride) = make_fb(2, 2);
        let mut fb = FrameBuffer {
            width: 2,
            height: 2,
            stride,
            pixels: &mut px,
        };

        fill_rect_raw(&mut fb, Rect::new(0, 0, 2, 2), Rgb::new(0x12, 0x34, 0x56));

        for chunk in fb.pixels.chunks_exact(4) {
            assert_eq!(chunk, &[0x56, 0x34, 0x12, 0xFF]);
        }
    }

    fn pixel_is_drawn(pixels: &[u8], stride: u32, x: u32, y: u32) -> bool {
        let idx = y as usize * stride as usize + x as usize * 4 + 3;
        pixels.get(idx).copied().unwrap_or_default() != 0
    }

    #[test]
    fn native_horizontal_box_lines_span_edge_to_edge() {
        let (mut px, stride) = make_fb(9, 9);
        let mut fb = FrameBuffer {
            width: 9,
            height: 9,
            stride,
            pixels: &mut px,
        };

        assert!(draw_native_box_char(
            &mut fb,
            Rect::new(0, 0, 9, 9),
            '─',
            Rgb::new(255, 255, 255),
        ));

        let filled_rows: Vec<u32> = (0..fb.height)
            .filter(|&y| (0..fb.width).any(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y)))
            .collect();
        assert_eq!(filled_rows.len(), 1);
        let y = filled_rows[0];
        assert!((0..fb.width).all(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y)));
    }

    #[test]
    fn native_corner_box_lines_meet_at_cell_center() {
        let (mut px, stride) = make_fb(9, 9);
        let mut fb = FrameBuffer {
            width: 9,
            height: 9,
            stride,
            pixels: &mut px,
        };

        assert!(draw_native_box_char(
            &mut fb,
            Rect::new(0, 0, 9, 9),
            '┌',
            Rgb::new(255, 255, 255),
        ));

        assert!(pixel_is_drawn(fb.pixels, fb.stride, 4, 4));
        assert!(pixel_is_drawn(fb.pixels, fb.stride, 8, 4));
        assert!(pixel_is_drawn(fb.pixels, fb.stride, 4, 8));
        assert!(!pixel_is_drawn(fb.pixels, fb.stride, 0, 4));
        assert!(!pixel_is_drawn(fb.pixels, fb.stride, 4, 0));
    }

    #[test]
    fn native_double_box_lines_draw_parallel_strokes() {
        let (mut px, stride) = make_fb(9, 9);
        let mut fb = FrameBuffer {
            width: 9,
            height: 9,
            stride,
            pixels: &mut px,
        };

        assert!(draw_native_box_char(
            &mut fb,
            Rect::new(0, 0, 9, 9),
            '═',
            Rgb::new(255, 255, 255),
        ));

        let filled_rows: Vec<u32> = (0..fb.height)
            .filter(|&y| (0..fb.width).any(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y)))
            .collect();
        assert_eq!(filled_rows.len(), 2);
        for y in filled_rows {
            assert!((0..fb.width).all(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y)));
        }
    }

    #[test]
    fn native_half_and_quadrant_blocks_match_claude_logo_chars() {
        let chars = ['▐', '▛', '█', '▜', '▌', '▝', '▘'];
        for ch in chars {
            let (mut px, stride) = make_fb(8, 8);
            let mut fb = FrameBuffer {
                width: 8,
                height: 8,
                stride,
                pixels: &mut px,
            };
            assert!(draw_native_block_char(
                &mut fb,
                Rect::new(0, 0, 8, 8),
                ch,
                Rgb::new(255, 255, 255),
            ));
            assert!(fb.pixels.chunks_exact(4).any(|chunk| chunk[3] != 0), "{ch}");
        }
    }

    #[test]
    fn native_right_half_block_fills_right_columns_only() {
        let (mut px, stride) = make_fb(8, 8);
        let mut fb = FrameBuffer {
            width: 8,
            height: 8,
            stride,
            pixels: &mut px,
        };

        assert!(draw_native_block_char(
            &mut fb,
            Rect::new(0, 0, 8, 8),
            '▐',
            Rgb::new(255, 255, 255),
        ));
        assert!((0..8).all(|y| (0..4).all(|x| !pixel_is_drawn(fb.pixels, fb.stride, x, y))));
        assert!((0..8).all(|y| (4..8).all(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y))));
    }

    #[test]
    fn native_three_quadrant_block_leaves_missing_quadrant_empty() {
        let (mut px, stride) = make_fb(8, 8);
        let mut fb = FrameBuffer {
            width: 8,
            height: 8,
            stride,
            pixels: &mut px,
        };

        assert!(draw_native_block_char(
            &mut fb,
            Rect::new(0, 0, 8, 8),
            '▛',
            Rgb::new(255, 255, 255),
        ));
        assert!((0..4).all(|y| (0..8).all(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y))));
        assert!((4..8).all(|y| (0..4).all(|x| pixel_is_drawn(fb.pixels, fb.stride, x, y))));
        assert!((4..8).all(|y| (4..8).all(|x| !pixel_is_drawn(fb.pixels, fb.stride, x, y))));
    }

    #[test]
    fn partial_render_preserves_undamaged_rows() {
        if std::panic::catch_unwind(crate::freetype_ffi::ft).is_err() {
            return;
        }

        let theme = Theme::default();
        let mut renderer = SoftwareRenderer::new(14.0, "monospace".to_string(), theme, 0);
        let (cols, rows) = renderer.grid_size_for_window(90, 36);
        let rows = rows.max(2);
        let mut grid = Grid::new(cols, rows);
        grid.cell_mut(0, 0).ch = 'A' as u32;
        grid.cell_mut(0, 1).ch = 'B' as u32;

        let mut damage = DamageTracker::new(grid.rows());
        damage.clear();
        damage.mark_row(0);

        let (fb_width, fb_height) = renderer.window_size_for_grid(cols, rows);
        let (mut px, stride) = make_fb(fb_width, fb_height);
        px.fill(0x7b);
        let mut fb = FrameBuffer {
            width: fb_width,
            height: fb_height,
            stride,
            pixels: &mut px,
        };

        let cursor = CursorRenderer::new();
        let selection = Selection::new();
        let dirty_rows = damage.dirty_row_ranges();
        renderer.render(
            &mut fb,
            RenderState {
                grid: &mut grid,
                cursor: &cursor,
                selection: &selection,
                dirty_rows: &dirty_rows,
            },
            RenderOptions {
                full_redraw: false,
                draw_cursor: false,
            },
        );

        let (dirty_y0, dirty_y1) = renderer.pixel_bounds_for_row_range(0, 0);
        let (_, clean_y1) = renderer.pixel_bounds_for_row_range(1, 1);

        assert!(fb.pixels
            [(dirty_y0 as usize * stride as usize)..(dirty_y1 as usize * stride as usize)]
            .iter()
            .any(|&byte| byte != 0x7b));
        assert!(fb.pixels
            [(dirty_y1 as usize * stride as usize)..(clean_y1 as usize * stride as usize)]
            .iter()
            .all(|&byte| byte == 0x7b));
    }

    #[test]
    fn axis_aligned_quad_fast_path_matches_expected_pixel_span() {
        assert_eq!(
            axis_aligned_quad_fill_bounds([(1.0, 2.0), (4.0, 2.0), (4.0, 5.0), (1.0, 5.0)], 10, 10,),
            Some((1, 2, 3, 3))
        );
        assert_eq!(
            axis_aligned_quad_fill_bounds([(1.2, 2.2), (4.8, 2.2), (4.8, 5.8), (1.2, 5.8)], 10, 10,),
            Some((1, 2, 4, 4))
        );
        assert_eq!(
            axis_aligned_quad_fill_bounds([(1.0, 2.0), (4.0, 2.1), (4.0, 5.0), (1.0, 5.0)], 10, 10,),
            None
        );
    }
}
