//! Machine-checkable contracts: preconditions, postconditions, and struct invariants.
//!
//! All checks compile to `debug_assert!` — zero cost in release builds, full
//! validation in debug/test builds.  Run `cargo test` to exercise every
//! contract and invariant.
//!
//! # Macros
//!
//! | Macro                      | Purpose                                    |
//! |----------------------------|--------------------------------------------|
//! | `requires!(cond)`          | Precondition on function entry             |
//! | `ensures!(cond)`           | Postcondition before function return       |
//! | `invariant!(cond)`         | Structural invariant (any point)           |
//! | `debug_check_invariant!(v)`| Call `v.check_invariant()` in debug builds |

/// Precondition — checked on function entry.
#[macro_export]
macro_rules! requires {
    ($cond:expr) => {
        debug_assert!($cond, "precondition violated: {}", stringify!($cond));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "precondition violated: {}", format_args!($($arg)*));
    };
}

/// Postcondition — checked before function return.
#[macro_export]
macro_rules! ensures {
    ($cond:expr) => {
        debug_assert!($cond, "postcondition violated: {}", stringify!($cond));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "postcondition violated: {}", format_args!($($arg)*));
    };
}

/// Structural invariant — checked at any point.
#[macro_export]
macro_rules! invariant {
    ($cond:expr) => {
        debug_assert!($cond, "invariant violated: {}", stringify!($cond));
    };
    ($cond:expr, $($arg:tt)*) => {
        debug_assert!($cond, "invariant violated: {}", format_args!($($arg)*));
    };
}

/// Trait for types with machine-checkable structural invariants.
///
/// Implementors define `check_invariant()` which panics (via `invariant!`)
/// if the struct is in an invalid state.  Called automatically by
/// `debug_check_invariant!` in debug builds.
#[allow(dead_code)]
pub trait CheckInvariant {
    fn check_invariant(&self);
}

// Blanket impl so &T and &mut T also satisfy the trait.
impl<T: CheckInvariant + ?Sized> CheckInvariant for &T {
    #[inline]
    fn check_invariant(&self) { (**self).check_invariant(); }
}
impl<T: CheckInvariant + ?Sized> CheckInvariant for &mut T {
    #[inline]
    fn check_invariant(&self) { (**self).check_invariant(); }
}

/// Call `check_invariant()` on a value — compiles to nothing in release.
/// Works with `self`, `&self`, `&mut self`, and owned values.
#[macro_export]
macro_rules! debug_check_invariant {
    ($val:expr) => {
        #[cfg(debug_assertions)]
        $crate::contracts::CheckInvariant::check_invariant(&$val);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Positive(i32);
    impl CheckInvariant for Positive {
        fn check_invariant(&self) {
            invariant!(self.0 > 0, "Positive must be > 0, got {}", self.0);
        }
    }

    #[test]
    fn requires_passes() {
        let x = 5;
        requires!(x > 0);
        requires!(x > 0, "x={} should be positive", x);
    }

    #[test]
    fn ensures_passes() {
        let result = 42;
        ensures!(result > 0);
        ensures!(result > 0, "result must be positive");
    }

    #[test]
    fn invariant_passes() {
        let p = Positive(1);
        debug_check_invariant!(p);
    }

    #[test]
    #[should_panic(expected = "precondition violated")]
    fn requires_fails() {
        requires!(false);
    }

    #[test]
    #[should_panic(expected = "postcondition violated")]
    fn ensures_fails() {
        ensures!(false);
    }

    #[test]
    #[should_panic(expected = "invariant violated")]
    fn invariant_fails() {
        let p = Positive(-1);
        debug_check_invariant!(p);
    }
}
