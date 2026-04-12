//! Cursor visual effects — particle trails and point highlights.
//! Ported from Neovide's cursor VFX, adapted for CPU software rendering.
//! All coordinates are grid-space (col/row floats); the renderer converts to pixels.

use std::f32::consts::{FRAC_PI_2, PI};

// ─── Easing helpers ──────────────────────────────────────────────────────────

#[inline]
fn ease_in_quad(t: f32) -> f32 {
    t * t
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn ease(start: f32, end: f32, t: f32) -> f32 {
    lerp(start, end, ease_in_quad(t))
}

// ─── Public types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfxMode {
    Disabled,
    Railgun,
    Torpedo,
    PixieDust,
    SonicBoom,
    Ripple,
    Wireframe,
}

pub fn parse_vfx_mode(s: &str) -> VfxMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "railgun" => VfxMode::Railgun,
        "torpedo" => VfxMode::Torpedo,
        "pixiedust" | "pixie_dust" => VfxMode::PixieDust,
        "sonicboom" | "sonic_boom" => VfxMode::SonicBoom,
        "ripple" => VfxMode::Ripple,
        "wireframe" => VfxMode::Wireframe,
        _ => VfxMode::Disabled,
    }
}

/// Parse a comma-separated list of VFX mode names into a `Vec<VfxMode>`.
/// Disabled entries and empty segments are filtered out.
pub fn parse_vfx_modes(s: &str) -> Vec<VfxMode> {
    s.split(',')
        .map(parse_vfx_mode)
        .filter(|m| *m != VfxMode::Disabled)
        .collect()
}

#[derive(Clone, Debug)]
pub enum ParticleShape {
    FilledOval,
    StrokedOval,
    FilledRect,
    StrokedRect,
}

#[derive(Clone, Debug)]
pub struct VfxParticle {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub alpha: u8,
    pub shape: ParticleShape,
    pub stroke_width: f32,
}

#[derive(Clone, Debug)]
pub struct VfxSettings {
    pub mode: Vec<VfxMode>,
    pub opacity: f32,
    pub particle_lifetime: f32,
    pub particle_highlight_lifetime: f32,
    pub particle_density: f32,
    pub particle_speed: f32,
    pub particle_phase: f32,
    pub particle_curl: f32,
}

impl Default for VfxSettings {
    fn default() -> Self {
        Self {
            mode: Vec::new(),
            opacity: 200.0,
            particle_lifetime: 0.5,
            particle_highlight_lifetime: 0.2,
            particle_density: 0.7,
            particle_speed: 10.0,
            particle_phase: 1.5,
            particle_curl: 1.0,
        }
    }
}

// ─── PCG random number generator ────────────────────────────────────────────

struct RngState {
    state: u64,
    inc: u64,
}

impl RngState {
    fn new() -> Self {
        Self {
            state: 0x853C_49E6_748F_EA9B,
            inc: (0xDA3E_39CB_94B9_5BDB << 1) | 1,
        }
    }

    fn next(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let rot = (old >> 59) as u32;
        let xsh = (((old >> 18) ^ old) >> 27) as u32;
        xsh.rotate_right(rot)
    }

    fn next_f32(&mut self) -> f32 {
        let v = self.next();
        let bits = (v as f64).to_bits();
        let exp = (bits >> 52) & ((1 << 11) - 1);
        let new_exp = exp.max(32) - 32;
        let new_bits = (new_exp << 52) | (bits & 0x801F_FFFF_FFFF_FFFF);
        f64::from_bits(new_bits) as f32
    }

    fn rand_dir(&mut self) -> (f32, f32) {
        let x = self.next_f32() * 2.0 - 1.0;
        let y = self.next_f32() * 2.0 - 1.0;
        (x, y)
    }

    fn rand_dir_normalized(&mut self) -> (f32, f32) {
        let (x, y) = self.rand_dir();
        let len = (x * x + y * y).sqrt();
        if len < 1e-6 {
            (1.0, 0.0)
        } else {
            (x / len, y / len)
        }
    }
}

fn rotate_vec(vx: f32, vy: f32, rot: f32) -> (f32, f32) {
    let (s, c) = rot.sin_cos();
    (vx * c - vy * s, vx * s + vy * c)
}

fn vec_length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn normalize(x: f32, y: f32) -> (f32, f32) {
    let len = vec_length(x, y);
    if len < 1e-6 {
        (0.0, 0.0)
    } else {
        (x / len, y / len)
    }
}

// ─── Point highlight ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
enum HighlightKind {
    SonicBoom,
    Ripple,
    Wireframe,
}

struct PointHighlight {
    t: f32,
    center: (f32, f32),
    kind: HighlightKind,
}

impl PointHighlight {
    fn new(kind: HighlightKind) -> Self {
        Self {
            t: 1.0,
            center: (0.0, 0.0),
            kind,
        }
    }

    fn update(&mut self, settings: &VfxSettings, dest: (f32, f32), dt: f32) -> bool {
        self.center = dest;
        let lifetime = settings.particle_highlight_lifetime;
        if lifetime > 0.0 {
            self.t = (self.t + dt / lifetime).min(1.0);
        } else {
            self.t = 1.0;
        }
        self.t < 1.0
    }

    fn restart(&mut self, pos: (f32, f32)) {
        self.t = 0.0;
        self.center = pos;
    }

    fn particles(&self, settings: &VfxSettings) -> Vec<VfxParticle> {
        if (self.t - 1.0).abs() < f32::EPSILON {
            return Vec::new();
        }
        let alpha = ease(settings.opacity, 0.0, self.t) as u8;
        let size = 3.0; // 3× cell height in grid-space
        let radius = self.t * size;
        let (shape, stroke_width) = match self.kind {
            HighlightKind::SonicBoom => (ParticleShape::FilledOval, 0.0),
            HighlightKind::Ripple => (ParticleShape::StrokedOval, 0.2),
            HighlightKind::Wireframe => (ParticleShape::StrokedRect, 0.2),
        };
        vec![VfxParticle {
            x: self.center.0,
            y: self.center.1,
            radius,
            alpha,
            shape,
            stroke_width,
        }]
    }
}

// ─── Particle trail ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
enum TrailKind {
    Railgun,
    Torpedo,
    PixieDust,
}

#[derive(Clone)]
struct ParticleData {
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    rotation_speed: f32,
    lifetime: f32,
}

struct ParticleTrail {
    particles: Vec<ParticleData>,
    prev_dest: (f32, f32),
    kind: TrailKind,
    rng: RngState,
    count_reminder: f32,
}

impl ParticleTrail {
    fn new(kind: TrailKind) -> Self {
        Self {
            particles: Vec::new(),
            prev_dest: (0.0, 0.0),
            kind,
            rng: RngState::new(),
            count_reminder: 0.0,
        }
    }

    fn update(
        &mut self,
        settings: &VfxSettings,
        dest: (f32, f32),
        _cursor_size: (f32, f32),
        dt: f32,
    ) -> bool {
        // Update lifetimes, remove dead
        let mut i = 0;
        while i < self.particles.len() {
            self.particles[i].lifetime -= dt;
            if self.particles[i].lifetime <= 0.0 {
                self.particles.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // Update positions + rotate speeds
        for p in &mut self.particles {
            p.x += p.sx * dt;
            p.y += p.sy * dt;
            let (rx, ry) = rotate_vec(p.sx, p.sy, dt * p.rotation_speed);
            p.sx = rx;
            p.sy = ry;
        }

        // Spawn new particles on movement
        if (dest.0 - self.prev_dest.0).abs() > 1e-4 || (dest.1 - self.prev_dest.1).abs() > 1e-4 {
            let travel_x = dest.0 - self.prev_dest.0;
            let travel_y = dest.1 - self.prev_dest.1;
            let travel_dist = vec_length(travel_x, travel_y);
            let cell_h = 1.0f32; // grid-space

            let f_count = (travel_dist / cell_h) * settings.particle_density + self.count_reminder;
            let count = f_count as usize;
            self.count_reminder = f_count - count as f32;

            let prev = self.prev_dest;

            for i in 0..count {
                let t = (i + 1) as f32 / count as f32;

                let (sx, sy) = match self.kind {
                    TrailKind::Railgun => {
                        let phase =
                            t / PI * settings.particle_phase * (travel_dist / cell_h);
                        (
                            phase.sin() * 2.0 * settings.particle_speed,
                            phase.cos() * 2.0 * settings.particle_speed,
                        )
                    }
                    TrailKind::Torpedo => {
                        let travel_dir = normalize(travel_x, travel_y);
                        let rd = self.rng.rand_dir_normalized();
                        let px = rd.0 - travel_dir.0 * 1.5;
                        let py = rd.1 - travel_dir.1 * 1.5;
                        let (nx, ny) = normalize(px, py);
                        (nx * settings.particle_speed, ny * settings.particle_speed)
                    }
                    TrailKind::PixieDust => {
                        let base = self.rng.rand_dir_normalized();
                        let dx = base.0 * 0.5;
                        let dy = 0.4 + base.1.abs();
                        (
                            dx * 3.0 * settings.particle_speed,
                            dy * 3.0 * settings.particle_speed,
                        )
                    }
                };

                let (px, py) = match self.kind {
                    TrailKind::Railgun => (prev.0 + travel_x * t, prev.1 + travel_y * t),
                    TrailKind::Torpedo | TrailKind::PixieDust => {
                        let r = self.rng.next_f32();
                        (
                            prev.0 + travel_x * r,
                            prev.1 + travel_y * r + cell_h * 0.5,
                        )
                    }
                };

                let rotation_speed = match self.kind {
                    TrailKind::Railgun => PI * settings.particle_curl,
                    TrailKind::Torpedo | TrailKind::PixieDust => {
                        (self.rng.next_f32() - 0.5) * FRAC_PI_2 * settings.particle_curl
                    }
                };

                self.particles.push(ParticleData {
                    x: px,
                    y: py,
                    sx,
                    sy,
                    rotation_speed,
                    lifetime: t * settings.particle_lifetime,
                });
            }

            self.prev_dest = dest;
        }

        !self.particles.is_empty()
    }

    #[allow(dead_code)]
    fn restart(&mut self) {
        self.count_reminder = 0.0;
    }

    fn vfx_particles(&self, settings: &VfxSettings) -> Vec<VfxParticle> {
        let cell_w = 1.0f32; // grid-space
        self.particles
            .iter()
            .map(|p| {
                let lifetime_frac = p.lifetime / settings.particle_lifetime;
                let alpha = (lifetime_frac * settings.opacity) as u8;
                let (radius, shape, stroke_width) = match self.kind {
                    TrailKind::Railgun | TrailKind::Torpedo => {
                        (cell_w * 0.5 * lifetime_frac, ParticleShape::StrokedOval, 0.2)
                    }
                    TrailKind::PixieDust => (cell_w * 0.2, ParticleShape::FilledRect, 0.0),
                };
                VfxParticle {
                    x: p.x,
                    y: p.y,
                    radius,
                    alpha,
                    shape,
                    stroke_width,
                }
            })
            .collect()
    }
}

// ─── Public facade ──────────────────────────────────────────────────────────

pub struct CursorVfx {
    settings: VfxSettings,
    highlights: Vec<PointHighlight>,
    trails: Vec<ParticleTrail>,
}

impl CursorVfx {
    pub fn new(settings: VfxSettings) -> Self {
        let (highlights, trails) = build_effects(&settings.mode);
        Self {
            settings,
            highlights,
            trails,
        }
    }

    pub fn set_settings(&mut self, settings: VfxSettings) {
        if settings.mode != self.settings.mode {
            let (highlights, trails) = build_effects(&settings.mode);
            self.highlights = highlights;
            self.trails = trails;
        }
        self.settings = settings;
    }

    /// Advance VFX state. Returns `true` if still animating.
    pub fn update(
        &mut self,
        cursor_dest: (f32, f32),
        cursor_size: (f32, f32),
        dt: f32,
    ) -> bool {
        if self.settings.mode.is_empty() {
            return false;
        }
        let mut animating = false;
        for h in &mut self.highlights {
            animating |= h.update(&self.settings, cursor_dest, dt);
        }
        for t in &mut self.trails {
            animating |= t.update(&self.settings, cursor_dest, cursor_size, dt);
        }
        animating
    }

    /// Notify that the cursor teleported (triggers highlight restart).
    pub fn cursor_jumped(&mut self, position: (f32, f32)) {
        for h in &mut self.highlights {
            h.restart(position);
        }
    }

    /// Collect all renderable particles for the current frame.
    pub fn particles(&self) -> Vec<VfxParticle> {
        if self.settings.mode.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for h in &self.highlights {
            out.extend(h.particles(&self.settings));
        }
        for t in &self.trails {
            out.extend(t.vfx_particles(&self.settings));
        }
        out
    }

    #[allow(dead_code)]
    pub fn modes(&self) -> &[VfxMode] {
        &self.settings.mode
    }
}

fn build_effects(modes: &[VfxMode]) -> (Vec<PointHighlight>, Vec<ParticleTrail>) {
    let mut highlights = Vec::new();
    let mut trails = Vec::new();
    for mode in modes {
        match mode {
            VfxMode::Disabled => {}
            VfxMode::Railgun => trails.push(ParticleTrail::new(TrailKind::Railgun)),
            VfxMode::Torpedo => trails.push(ParticleTrail::new(TrailKind::Torpedo)),
            VfxMode::PixieDust => trails.push(ParticleTrail::new(TrailKind::PixieDust)),
            VfxMode::SonicBoom => highlights.push(PointHighlight::new(HighlightKind::SonicBoom)),
            VfxMode::Ripple => highlights.push(PointHighlight::new(HighlightKind::Ripple)),
            VfxMode::Wireframe => highlights.push(PointHighlight::new(HighlightKind::Wireframe)),
        }
    }
    (highlights, trails)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_deterministic() {
        let mut a = RngState::new();
        let mut b = RngState::new();
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn rng_f32_range() {
        let mut rng = RngState::new();
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!(v >= 0.0 && v < 1.0, "got {v}");
        }
    }

    #[test]
    fn default_settings() {
        let s = VfxSettings::default();
        assert!(s.mode.is_empty());
        assert!((s.opacity - 200.0).abs() < f32::EPSILON);
        assert!((s.particle_lifetime - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_modes() {
        assert_eq!(parse_vfx_mode("railgun"), VfxMode::Railgun);
        assert_eq!(parse_vfx_mode("torpedo"), VfxMode::Torpedo);
        assert_eq!(parse_vfx_mode("pixiedust"), VfxMode::PixieDust);
        assert_eq!(parse_vfx_mode("pixie_dust"), VfxMode::PixieDust);
        assert_eq!(parse_vfx_mode("sonicboom"), VfxMode::SonicBoom);
        assert_eq!(parse_vfx_mode("sonic_boom"), VfxMode::SonicBoom);
        assert_eq!(parse_vfx_mode("ripple"), VfxMode::Ripple);
        assert_eq!(parse_vfx_mode("wireframe"), VfxMode::Wireframe);
        assert_eq!(parse_vfx_mode(""), VfxMode::Disabled);
        assert_eq!(parse_vfx_mode("nonsense"), VfxMode::Disabled);
        assert_eq!(parse_vfx_mode("RAILGUN"), VfxMode::Railgun);
    }

    #[test]
    fn disabled_produces_no_particles() {
        let mut vfx = CursorVfx::new(VfxSettings::default());
        vfx.update((5.0, 5.0), (1.0, 1.0), 0.016);
        assert!(vfx.particles().is_empty());
    }

    #[test]
    fn parse_vfx_modes_comma_separated() {
        let modes = parse_vfx_modes("railgun,ripple");
        assert_eq!(modes, vec![VfxMode::Railgun, VfxMode::Ripple]);
    }

    #[test]
    fn parse_vfx_modes_empty_string() {
        let modes = parse_vfx_modes("");
        assert!(modes.is_empty());
    }

    #[test]
    fn parse_vfx_modes_filters_disabled() {
        let modes = parse_vfx_modes("railgun,nonsense,torpedo");
        assert_eq!(modes, vec![VfxMode::Railgun, VfxMode::Torpedo]);
    }

    #[test]
    fn highlight_lifecycle() {
        let settings = VfxSettings {
            mode: vec![VfxMode::SonicBoom],
            ..VfxSettings::default()
        };
        let mut vfx = CursorVfx::new(settings);
        vfx.cursor_jumped((5.0, 5.0));
        assert!(vfx.update((5.0, 5.0), (1.0, 1.0), 0.001));
        let p = vfx.particles();
        assert!(!p.is_empty());
        // After enough time, should expire
        for _ in 0..200 {
            vfx.update((5.0, 5.0), (1.0, 1.0), 0.016);
        }
        assert!(vfx.particles().is_empty());
    }

    #[test]
    fn trail_spawns_on_movement() {
        let settings = VfxSettings {
            mode: vec![VfxMode::Railgun],
            ..VfxSettings::default()
        };
        let mut vfx = CursorVfx::new(settings);
        // First update sets prev_dest and may spawn from (0,0)→(5,5)
        vfx.update((5.0, 5.0), (1.0, 1.0), 0.016);
        let _initial = vfx.particles().len();
        // Let initial particles die
        for _ in 0..200 {
            vfx.update((5.0, 5.0), (1.0, 1.0), 0.016);
        }
        assert!(vfx.particles().is_empty(), "particles should decay");
        // Move cursor → new particles spawn
        vfx.update((10.0, 5.0), (1.0, 1.0), 0.016);
        assert!(!vfx.particles().is_empty());
    }

    #[test]
    fn torpedo_spawns_on_movement() {
        let settings = VfxSettings {
            mode: vec![VfxMode::Torpedo],
            ..VfxSettings::default()
        };
        let mut vfx = CursorVfx::new(settings);
        vfx.update((0.0, 0.0), (1.0, 1.0), 0.016);
        vfx.update((5.0, 0.0), (1.0, 1.0), 0.016);
        assert!(!vfx.particles().is_empty());
    }

    #[test]
    fn pixiedust_spawns_on_movement() {
        let settings = VfxSettings {
            mode: vec![VfxMode::PixieDust],
            ..VfxSettings::default()
        };
        let mut vfx = CursorVfx::new(settings);
        vfx.update((0.0, 0.0), (1.0, 1.0), 0.016);
        vfx.update((5.0, 0.0), (1.0, 1.0), 0.016);
        assert!(!vfx.particles().is_empty());
    }

    #[test]
    fn mode_switch_rebuilds_effects() {
        let mut vfx = CursorVfx::new(VfxSettings {
            mode: vec![VfxMode::Railgun],
            ..VfxSettings::default()
        });
        assert_eq!(vfx.trails.len(), 1);
        assert!(vfx.highlights.is_empty());
        vfx.set_settings(VfxSettings {
            mode: vec![VfxMode::Ripple],
            ..VfxSettings::default()
        });
        assert!(vfx.trails.is_empty());
        assert_eq!(vfx.highlights.len(), 1);
    }

    #[test]
    fn particles_decay_over_time() {
        let settings = VfxSettings {
            mode: vec![VfxMode::Railgun],
            ..VfxSettings::default()
        };
        let mut vfx = CursorVfx::new(settings);
        vfx.update((0.0, 0.0), (1.0, 1.0), 0.016);
        vfx.update((10.0, 0.0), (1.0, 1.0), 0.016);
        let initial = vfx.particles().len();
        assert!(initial > 0);
        // Let particles die
        for _ in 0..100 {
            vfx.update((10.0, 0.0), (1.0, 1.0), 0.016);
        }
        assert!(vfx.particles().len() < initial);
    }
}
