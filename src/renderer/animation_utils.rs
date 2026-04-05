use glamour::{Point2, Unit};

#[allow(dead_code)]
pub fn ease_out_expo(t: f32) -> f32 {
    if (t - 1.0).abs() < f32::EPSILON { 1.0 } else { 1.0 - 2.0f32.powf(-10.0 * t) }
}

pub fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t
}

pub fn ease(ease_func: fn(f32) -> f32, start: f32, end: f32, t: f32) -> f32 {
    lerp(start, end, ease_func(t))
}

pub fn ease_point<T: Unit<Scalar = f32>>(
    ease_func: fn(f32) -> f32,
    start: Point2<T>,
    end: Point2<T>,
    t: f32,
) -> Point2<T> {
    Point2::new(ease(ease_func, start.x, end.x, t), ease(ease_func, start.y, end.y, t))
}

#[derive(Clone)]
pub struct CriticallyDampedSpringAnimation {
    pub position: f32,
    velocity: f32,
}

impl CriticallyDampedSpringAnimation {
    pub fn new() -> Self {
        Self { position: 0.0, velocity: 0.0 }
    }

    pub fn update(&mut self, dt: f32, animation_length: f32) -> bool {
        if animation_length <= dt {
            self.reset();
            return false;
        }
        if self.position == 0.0 {
            return false;
        }

        // Simulate a critically damped spring, also known as a PD controller.
        // For more details of why this was chosen, see this:
        // https://gdcvault.com/play/1027059/Math-In-Game-Development-Summit
        // < 1 underdamped,  1 critically damped, > 1 overdamped
        let zeta = 1.0;
        // The omega is calculated so that the destination is reached with a 2% tolerance in
        // animation_length time.
        let omega = 4.0 / (zeta * animation_length);

        // Use the analytica formula for critically damped harmonic oscillation
        // a and b are the intial conditions by setting dt to zero and solving the position and
        // velocity respectively
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_lerp() {
        assert_eq!(lerp(1.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn test_ease_out_expo() {
        assert_eq!(ease(ease_out_expo, 1.0, 0.0, 1.0), 0.0);
        assert_eq!(ease(ease_out_expo, 1.0, 0.0, 1.1), 0.00048828125);
    }
}
