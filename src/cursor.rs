//! Critically-damped spring cursor animation.
//! Ported from termvide's cursor renderer.

use crate::defaults::{
    DEFAULT_CURSOR_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_BLINK_INTERVAL_SECS,
    DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_TRAIL_SIZE,
};

const CRITICAL_DAMPING_RATIO: f32 = 1.0;
const SPRING_DAMPING_FACTOR: f32 = 4.0;
const SPRING_SETTLE_THRESHOLD: f32 = 0.01;
const POSITION_CHANGE_EPSILON: f32 = 0.001;
const DESTINATION_CHANGE_EPSILON: f32 = 0.001;
const MIN_CURSOR_HALF_SIZE: f32 = 0.02;
const MAX_CURSOR_HALF_SIZE: f32 = 0.5;
const SHORT_MOVE_THRESHOLD_COLS: f32 = 2.001;
const CURSOR_CELL_CENTER: f32 = 0.5;
const INITIAL_PREVIOUS_POSITION: f32 = -1000.0;
const CELL_DIMENSION: f32 = 1.0;

#[derive(Clone, Debug)]
pub(crate) struct CriticallyDampedSpring {
    position: f32,
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
        let omega = SPRING_DAMPING_FACTOR / (CRITICAL_DAMPING_RATIO * animation_length);
        let a = self.position;
        let b = self.position * omega + self.velocity;
        let c = (-omega * dt).exp();
        self.position = (a + b * dt) * c;
        self.velocity = c * (-a * omega - b * dt * omega + b);
        if self.position.abs() < SPRING_SETTLE_THRESHOLD {
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

const STANDARD_CORNERS: [(f32, f32); 4] = [
    (-CURSOR_CELL_CENTER, -CURSOR_CELL_CENTER),
    (CURSOR_CELL_CENTER, -CURSOR_CELL_CENTER),
    (CURSOR_CELL_CENTER, CURSOR_CELL_CENTER),
    (-CURSOR_CELL_CENTER, CURSOR_CELL_CENTER),
];
const BEAM_WIDTH_CELLS: f32 = 0.12;
const UNDERLINE_HEIGHT_CELLS: f32 = 0.16;

#[derive(Clone, Debug, PartialEq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Debug)]
pub(crate) struct Corner {
    current_x: f32,
    current_y: f32,
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
            prev_dest_x: INITIAL_PREVIOUS_POSITION,
            prev_dest_y: INITIAL_PREVIOUS_POSITION,
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
        if (dest_x - self.prev_dest_x).abs() > DESTINATION_CHANGE_EPSILON
            || (dest_y - self.prev_dest_y).abs() > DESTINATION_CHANGE_EPSILON
        {
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
        if len < POSITION_CHANGE_EPSILON {
            return 0.0;
        }
        let rx = self.relative_x;
        let ry = self.relative_y;
        let rlen = (rx * rx + ry * ry).sqrt();
        if rlen < POSITION_CHANGE_EPSILON {
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
                let half_width = (BEAM_WIDTH_CELLS * CURSOR_CELL_CENTER)
                    .clamp(MIN_CURSOR_HALF_SIZE, MAX_CURSOR_HALF_SIZE);
                self.relative_x = if sx < 0.0 {
                    -CURSOR_CELL_CENTER
                } else {
                    -CURSOR_CELL_CENTER + half_width * 2.0
                };
                self.relative_y = sy;
            }
            CursorShape::Underline => {
                let half_height = (UNDERLINE_HEIGHT_CELLS * CURSOR_CELL_CENTER)
                    .clamp(MIN_CURSOR_HALF_SIZE, MAX_CURSOR_HALF_SIZE);
                self.relative_x = sx;
                self.relative_y = if sy < 0.0 {
                    CURSOR_CELL_CENTER - half_height * 2.0
                } else {
                    CURSOR_CELL_CENTER
                };
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CursorSettings {
    animation_length: f32,
    short_animation_length: f32,
    trail_size: f32,
    blink_interval: f32,
}

impl Default for CursorSettings {
    fn default() -> Self {
        Self {
            animation_length: DEFAULT_CURSOR_ANIMATION_LENGTH_SECS,
            short_animation_length: DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS,
            trail_size: DEFAULT_CURSOR_TRAIL_SIZE,
            blink_interval: DEFAULT_CURSOR_BLINK_INTERVAL_SECS,
        }
    }
}

pub struct CursorRenderer {
    corners: [Corner; 4],
    pub(crate) shape: CursorShape,
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

    pub fn set_short_animation_length(&mut self, animation_length: f32) {
        self.settings.short_animation_length = animation_length.max(0.0);
    }

    pub fn set_trail_size(&mut self, trail_size: f32) {
        self.settings.trail_size = trail_size;
    }

    pub fn set_blink_enabled(&mut self, blink_enabled: bool) {
        self.blink_enabled = blink_enabled;
        self.blink_on = true;
        self.blink_timer = 0.0;
    }

    pub fn set_blink_interval(&mut self, blink_interval: f32) {
        self.settings.blink_interval = blink_interval.max(0.0);
        self.blink_timer = 0.0;
    }

    pub fn is_visible(&self) -> bool {
        !self.blink_enabled || self.blink_on
    }

    pub fn animate(&mut self, cursor_pos: (f32, f32), dt: f32) -> bool {
        let (col, row) = cursor_pos;
        let moved = (col - self.prev_col).abs() > POSITION_CHANGE_EPSILON
            || (row - self.prev_row).abs() > POSITION_CHANGE_EPSILON;
        if moved {
            self.jumped = true;
            self.blink_on = true;
            self.blink_timer = 0.0;
        }
        if self.blink_enabled {
            self.blink_timer += dt.max(0.0);
            while self.settings.blink_interval > 0.0
                && self.blink_timer >= self.settings.blink_interval
            {
                self.blink_timer -= self.settings.blink_interval;
                self.blink_on = !self.blink_on;
            }
        } else {
            self.blink_on = true;
            self.blink_timer = 0.0;
        }
        let center_x = col + CURSOR_CELL_CENTER;
        let center_y = row + CURSOR_CELL_CENTER;
        if self.jumped {
            let mut alignments: [(usize, f32); 4] = [
                (
                    0,
                    self.corners[0].direction_alignment(
                        center_x,
                        center_y,
                        CELL_DIMENSION,
                        CELL_DIMENSION,
                    ),
                ),
                (
                    1,
                    self.corners[1].direction_alignment(
                        center_x,
                        center_y,
                        CELL_DIMENSION,
                        CELL_DIMENSION,
                    ),
                ),
                (
                    2,
                    self.corners[2].direction_alignment(
                        center_x,
                        center_y,
                        CELL_DIMENSION,
                        CELL_DIMENSION,
                    ),
                ),
                (
                    3,
                    self.corners[3].direction_alignment(
                        center_x,
                        center_y,
                        CELL_DIMENSION,
                        CELL_DIMENSION,
                    ),
                ),
            ];
            alignments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut ranks = [0usize; 4];
            for (rank, &(idx, _)) in alignments.iter().enumerate() {
                ranks[idx] = rank;
            }
            let is_short = (col - self.prev_col).abs() <= SHORT_MOVE_THRESHOLD_COLS
                && (row - self.prev_row).abs() < POSITION_CHANGE_EPSILON;
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
            animating |= corner.update(
                center_x,
                center_y,
                CELL_DIMENSION,
                CELL_DIMENSION,
                dt,
                false,
            );
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
