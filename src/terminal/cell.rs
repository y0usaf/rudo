//! Compact terminal cell representation.
//! Inspired by foot's 12-byte cell design for cache efficiency.

use crate::contracts::CheckInvariant;
use crate::defaults::{DEFAULT_BACKGROUND_RGB, DEFAULT_FOREGROUND_RGB};

/// Cell text attributes packed into 2 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellFlags(u16);

impl CellFlags {
    pub const BOLD: Self = Self(1 << 0);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINE: Self = Self(1 << 3);
    pub const STRIKETHROUGH: Self = Self(1 << 4);
    pub const BLINK: Self = Self(1 << 5);
    pub const REVERSE: Self = Self(1 << 6);
    pub const HIDDEN: Self = Self(1 << 7);
    pub const WIDE: Self = Self(1 << 8);
    pub const WIDE_SPACER: Self = Self(1 << 9);
    pub const DIRTY: Self = Self(1 << 10);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }
    #[inline]
    pub const fn bits(self) -> u16 {
        self.0
    }
    #[inline]
    pub const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits)
    }
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
    #[inline]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitAnd for CellFlags {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitOr for CellFlags {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::Not for CellFlags {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        Self(!self.0)
    }
}

/// Color source - how fg/bg was specified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ColorSource {
    #[default]
    Default = 0,
    Palette = 1,
    Rgb = 2,
}

/// Packed RGB color (24-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackedColor(pub u32);

impl PackedColor {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }

    pub const fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub const fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub const fn b(self) -> u8 {
        self.0 as u8
    }
}

/// A single terminal cell - kept compact for cache performance.
/// 16 bytes total (char32 + flags + fg + bg).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Cell {
    /// Unicode codepoint (or combined chars index if > 0x200000).
    pub ch: u32,
    /// Cell attribute flags.
    pub flags: CellFlags,
    /// Foreground color source.
    pub fg_src: ColorSource,
    /// Background color source.
    pub bg_src: ColorSource,
    /// Foreground color (24-bit packed).
    pub fg: PackedColor,
    /// Background color (24-bit packed).
    pub bg: PackedColor,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ' as u32,
            flags: CellFlags::empty(),
            fg_src: ColorSource::Default,
            bg_src: ColorSource::Default,
            fg: PackedColor(DEFAULT_FOREGROUND_RGB),
            bg: PackedColor(DEFAULT_BACKGROUND_RGB),
        }
    }
}

impl Cell {
    pub fn character(&self) -> char {
        char::from_u32(self.ch).unwrap_or(' ')
    }

    pub fn mark_dirty(&mut self) {
        self.flags.insert(CellFlags::DIRTY);
    }

    pub fn clear_dirty(&mut self) {
        self.flags.remove(CellFlags::DIRTY);
    }
}

impl CheckInvariant for Cell {
    fn check_invariant(&self) {
        invariant!(
            !(self.flags.contains(CellFlags::WIDE) && self.flags.contains(CellFlags::WIDE_SPACER)),
            "Cell cannot be both WIDE and WIDE_SPACER"
        );
        invariant!(
            self.fg.0 & 0xFF00_0000 == 0 && self.bg.0 & 0xFF00_0000 == 0,
            "PackedColor upper byte must be zero"
        );
    }
}

// Verify our cell is compact
const _: () = assert!(std::mem::size_of::<Cell>() == 16);
