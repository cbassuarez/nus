//! A terminal cell: one column of one row.

use bitflags::bitflags;

/// A color as the application asked for it. Resolution to RGB happens at
/// render time against the current palette, so palette changes (OSC 4/10/11)
/// don't require rewriting the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// The terminal's default foreground or background (context-dependent).
    #[default]
    Default,
    /// One of the 256 palette entries. 0–15 are the ANSI colors.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Flags: u16 {
        const BOLD          = 1 << 0;
        const DIM           = 1 << 1;
        const ITALIC        = 1 << 2;
        const UNDERLINE     = 1 << 3;
        const DOUBLE_UL     = 1 << 4;
        const UNDERCURL     = 1 << 5;
        const DOTTED_UL     = 1 << 6;
        const DASHED_UL     = 1 << 7;
        const BLINK         = 1 << 8;
        const INVERSE       = 1 << 9;
        const HIDDEN        = 1 << 10;
        const STRIKE        = 1 << 11;
        /// First column of a double-width character.
        const WIDE          = 1 << 12;
        /// Second column of a double-width character; renders nothing.
        const WIDE_SPACER   = 1 << 13;

        const ANY_UNDERLINE = Self::UNDERLINE.bits() | Self::DOUBLE_UL.bits()
            | Self::UNDERCURL.bits() | Self::DOTTED_UL.bits() | Self::DASHED_UL.bits();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    /// Underline color; `None` means "same as fg".
    pub ul: Option<Color>,
    pub flags: Flags,
    /// OSC 8 hyperlink id into [`crate::Term::hyperlinks`]; 0 = none.
    pub link: u32,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            ul: None,
            flags: Flags::empty(),
            link: 0,
        }
    }
}

impl Cell {
    /// A blank cell carrying only the background of `template` — what erase
    /// operations leave behind (the "background color erase" behaviour every
    /// modern terminal has).
    pub fn erased_from(template: &Cell) -> Cell {
        Cell {
            bg: template.bg,
            ..Cell::default()
        }
    }

    pub fn is_blank(&self) -> bool {
        self.ch == ' '
            && self.bg == Color::Default
            && !self.flags.intersects(
                Flags::WIDE_SPACER | Flags::ANY_UNDERLINE | Flags::STRIKE | Flags::INVERSE,
            )
    }
}
