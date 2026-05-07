//! Cell attributes: foreground/background color + SGR flags.
//!
//! Compact representation matters: a 500×500 grid plus a 10k-line scrollback
//! pushes us into millions of cells. Storing `Attrs` inline per cell would
//! cost ~20 bytes/cell; instead the grid stores an `AttrId` (u16 index into
//! a flyweight table) and most cells share the same default entry.

use serde::{Deserialize, Serialize};

/// Terminal color. Three variants packed into a single u32 to keep `Attrs`
/// `Copy + Eq + Hash` (so we can use it as a HashMap key in the flyweight).
///
/// Tag in the high byte:
///   0x00 = Default        (terminal's configured fg/bg)
///   0x01 = Indexed(u8)    (palette index 0..=255 — covers ANSI 16, 6×6×6 cube, 24 grays)
///   0x02 = Rgb(r,g,b)     (truecolor; r in bits 16..24, g 8..16, b 0..8)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color(u32);

impl Color {
    pub const DEFAULT: Color = Color(0x0000_0000);

    pub const fn indexed(idx: u8) -> Color {
        Color(0x0100_0000 | idx as u32)
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(0x0200_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub fn kind(self) -> ColorKind {
        match self.0 >> 24 {
            0 => ColorKind::Default,
            1 => ColorKind::Indexed((self.0 & 0xff) as u8),
            2 => ColorKind::Rgb(
                ((self.0 >> 16) & 0xff) as u8,
                ((self.0 >> 8) & 0xff) as u8,
                (self.0 & 0xff) as u8,
            ),
            _ => ColorKind::Default,
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKind {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// SGR flag bits. `bitflags!` would work but adds a dep — a hand-rolled
/// `u16` is fine for 9 flags and keeps compile times down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Flags(u16);

impl Flags {
    pub const BOLD: Flags = Flags(1 << 0);
    pub const DIM: Flags = Flags(1 << 1);
    pub const ITALIC: Flags = Flags(1 << 2);
    pub const UNDERLINE: Flags = Flags(1 << 3);
    pub const BLINK: Flags = Flags(1 << 4);
    pub const INVERSE: Flags = Flags(1 << 5);
    pub const HIDDEN: Flags = Flags(1 << 6);
    pub const STRIKETHROUGH: Flags = Flags(1 << 7);
    pub const DBL_UNDERLINE: Flags = Flags(1 << 8);

    pub const fn empty() -> Self {
        Flags(0)
    }
    pub fn contains(self, other: Flags) -> bool {
        (self.0 & other.0) == other.0
    }
    pub fn insert(&mut self, other: Flags) {
        self.0 |= other.0;
    }
    pub fn remove(&mut self, other: Flags) {
        self.0 &= !other.0;
    }
    pub fn bits(self) -> u16 {
        self.0
    }
}

/// Full per-cell graphical attributes. `Eq + Hash` so the flyweight table
/// can dedupe identical attribute sets across millions of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Attrs {
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

impl Attrs {
    pub const DEFAULT: Attrs = Attrs {
        fg: Color::DEFAULT,
        bg: Color::DEFAULT,
        flags: Flags::empty(),
    };
}
