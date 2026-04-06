//! Critically-damped spring cursor animation.
//! Ported from termvide's cursor renderer.

#[derive(Clone, Debug)]
pub struct CriticallyDampedSpring {
    pub position: f32,
    velocity: f32,
}

impl CriticallyDampedSpring {
    pub fn new() -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32, animation_length: f32) -> bool {
        if animation_length <= dt {
            self.reset();
            return false;
        }
        if self.position == 0.0 {
            return false;
        }
        let zeta = 1.0;
        let omega = 4.0 / (zeta * animation_length);
        let a = self.position;
        let b = self.position * omega + self.velocity;
        let c = (-omega * dt).exp();
        self.position = (a + b * dt) * c;
        self.velocity = c * (-a * omega - b * dt * omega + b);
        if self.position.abs() < 0.01 {
            self.reset();
            false
        } else {
            true
        }
    }

    pub fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
    }
}

const STANDARD_CORNERS: [(f32, f32); 4] = [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)];
const BEAM_WIDTH_CELLS: f32 = 0.12;
const UNDERLINE_HEIGHT_CELLS: f32 = 0.16;

#[derive(Clone, Debug, PartialEq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Debug)]
pub struct Corner {
    pub current_x: f32,
    pub current_y: f32,
    relative_x: f32,
    relative_y: f32,
    prev_dest_x: f32,
    prev_dest_y: f32,
    spring_x: CriticallyDampedSpring,
    spring_y: CriticallyDampedSpring,
    animation_length: f32,
}

impl Corner {
    fn new(rel_x: f32, rel_y: f32) -> Self {
        Self {
            current_x: 0.0,
            current_y: 0.0,
            relative_x: rel_x,
            relative_y: rel_y,
            prev_dest_x: -1000.0,
            prev_dest_y: -1000.0,
            spring_x: CriticallyDampedSpring::new(),
            spring_y: CriticallyDampedSpring::new(),
            animation_length: 0.0,
        }
    }

    fn update(
        &mut self,
        center_x: f32,
        center_y: f32,
        cell_w: f32,
        cell_h: f32,
        dt: f32,
        immediate: bool,
    ) -> bool {
        let dest_x = center_x + self.relative_x * cell_w;
        let dest_y = center_y + self.relative_y * cell_h;
        if (dest_x - self.prev_dest_x).abs() > 0.001 || (dest_y - self.prev_dest_y).abs() > 0.001 {
            self.spring_x.position = dest_x - self.current_x;
            self.spring_y.position = dest_y - self.current_y;
            self.prev_dest_x = dest_x;
            self.prev_dest_y = dest_y;
        }
        if immediate {
            self.current_x = dest_x;
            self.current_y = dest_y;
            self.spring_x.reset();
            self.spring_y.reset();
            return false;
        }
        let mut animating = self.spring_x.update(dt, self.animation_length);
        animating |= self.spring_y.update(dt, self.animation_length);
        self.current_x = dest_x - self.spring_x.position;
        self.current_y = dest_y - self.spring_y.position;
        animating
    }

    fn direction_alignment(&self, center_x: f32, center_y: f32, cell_w: f32, cell_h: f32) -> f32 {
        let dest_x = center_x + self.relative_x * cell_w;
        let dest_y = center_y + self.relative_y * cell_h;
        let dx = dest_x - self.current_x;
        let dy = dest_y - self.current_y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            return 0.0;
        }
        let rx = self.relative_x;
        let ry = self.relative_y;
        let rlen = (rx * rx + ry * ry).sqrt();
        if rlen < 0.001 {
            return 0.0;
        }
        (dx / len) * (rx / rlen) + (dy / len) * (ry / rlen)
    }

    fn set_shape(&mut self, shape: &CursorShape, idx: usize) {
        let (sx, sy) = STANDARD_CORNERS[idx];
        match shape {
            CursorShape::Block => {
                self.relative_x = sx;
                self.relative_y = sy;
            }
            CursorShape::Beam => {
                let half_width = (BEAM_WIDTH_CELLS * 0.5).clamp(0.02, 0.5);
                self.relative_x = if sx < 0.0 {
                    -0.5
                } else {
                    -0.5 + half_width * 2.0
                };
                self.relative_y = sy;
            }
            CursorShape::Underline => {
                let half_height = (UNDERLINE_HEIGHT_CELLS * 0.5).clamp(0.02, 0.5);
                self.relative_x = sx;
                self.relative_y = if sy < 0.0 {
                    0.5 - half_height * 2.0
                } else {
                    0.5
                };
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CursorSettings {
    pub animation_length: f32,
    pub short_animation_length: f32,
    pub trail_size: f32,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            animation_length: 0.150,
            short_animation_length: 0.04,
            trail_size: 1.0,
        }
    }
}

pub struct CursorRenderer {
    pub corners: [Corner; 4],
    pub shape: CursorShape,
    settings: CursorSettings,
    prev_col: f32,
    prev_row: f32,
    jumped: bool,
    blink_on: bool,
    blink_enabled: bool,
    blink_timer: f32,
}

impl CursorRenderer {
    pub fn new() -> Self {
        Self {
            corners: [
                Corner::new(STANDARD_CORNERS[0].0, STANDARD_CORNERS[0].1),
                Corner::new(STANDARD_CORNERS[1].0, STANDARD_CORNERS[1].1),
                Corner::new(STANDARD_CORNERS[2].0, STANDARD_CORNERS[2].1),
                Corner::new(STANDARD_CORNERS[3].0, STANDARD_CORNERS[3].1),
            ],
            shape: CursorShape::Block,
            settings: CursorSettings::default(),
            prev_col: -1.0,
            prev_row: -1.0,
            jumped: false,
            blink_on: true,
            blink_enabled: false,
            blink_timer: 0.0,
        }
    }

    pub fn set_shape(&mut self, shape: CursorShape) {
        for (i, corner) in self.corners.iter_mut().enumerate() {
            corner.set_shape(&shape, i);
        }
        self.shape = shape;
    }

    pub fn set_animation_length(&mut self, animation_length: f32) {
        self.settings.animation_length = animation_length.max(0.0);
    }

    pub fn set_trail_size(&mut self, trail_size: f32) {
        self.settings.trail_size = trail_size;
    }

    pub fn set_blink_enabled(&mut self, blink_enabled: bool) {
        self.blink_enabled = blink_enabled;
        self.blink_on = true;
        self.blink_timer = 0.0;
    }

    pub fn is_visible(&self) -> bool {
        !self.blink_enabled || self.blink_on
    }

    pub fn animate(&mut self, cursor_pos: (f32, f32), dt: f32) -> bool {
        let (col, row) = cursor_pos;
        let moved = (col - self.prev_col).abs() > 0.001 || (row - self.prev_row).abs() > 0.001;
        if moved {
            self.jumped = true;
            self.blink_on = true;
            self.blink_timer = 0.0;
        }
        if self.blink_enabled {
            self.blink_timer += dt.max(0.0);
            while self.blink_timer >= 0.6 {
                self.blink_timer -= 0.6;
                self.blink_on = !self.blink_on;
            }
        } else {
            self.blink_on = true;
            self.blink_timer = 0.0;
        }
        let cell_w = 1.0;
        let cell_h = 1.0;
        let center_x = col + 0.5;
        let center_y = row + 0.5;
        if self.jumped {
            let mut alignments: [(usize, f32); 4] = [
                (
                    0,
                    self.corners[0].direction_alignment(center_x, center_y, cell_w, cell_h),
                ),
                (
                    1,
                    self.corners[1].direction_alignment(center_x, center_y, cell_w, cell_h),
                ),
                (
                    2,
                    self.corners[2].direction_alignment(center_x, center_y, cell_w, cell_h),
                ),
                (
                    3,
                    self.corners[3].direction_alignment(center_x, center_y, cell_w, cell_h),
                ),
            ];
            alignments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut ranks = [0usize; 4];
            for (rank, &(idx, _)) in alignments.iter().enumerate() {
                ranks[idx] = rank;
            }
            let is_short =
                (col - self.prev_col).abs() <= 2.001 && (row - self.prev_row).abs() < 0.001;
            for (i, corner) in self.corners.iter_mut().enumerate() {
                corner.animation_length = if is_short {
                    self.settings
                        .animation_length
                        .min(self.settings.short_animation_length)
                } else {
                    let leading = self.settings.animation_length
                        * (1.0 - self.settings.trail_size).clamp(0.0, 1.0);
                    let trailing = self.settings.animation_length;
                    match ranks[i] {
                        2..=3 => leading,
                        1 => (leading + trailing) / 2.0,
                        0 => trailing,
                        _ => trailing,
                    }
                };
            }
        }
        self.prev_col = col;
        self.prev_row = row;
        let mut animating = false;
        for corner in &mut self.corners {
            animating |= corner.update(center_x, center_y, 1.0, 1.0, dt, false);
        }
        self.jumped = false;
        animating || self.blink_enabled
    }

    pub fn corner_positions(&self) -> [(f32, f32); 4] {
        [
            (self.corners[0].current_x, self.corners[0].current_y),
            (self.corners[1].current_x, self.corners[1].current_y),
            (self.corners[2].current_x, self.corners[2].current_y),
            (self.corners[3].current_x, self.corners[3].current_y),
        ]
    }
}
