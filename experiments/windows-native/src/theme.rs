//! DBM's dark workbench palette and metrics, in device-independent pixels.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub const BG: u32 = 0x0b1017;
pub const SIDEBAR_BG: u32 = 0x0f1722;
pub const PANEL: u32 = 0x111924;
pub const PANEL_RAISED: u32 = 0x172231;
pub const PANEL_HOVER: u32 = 0x1b2a3d;
pub const EDITOR_BG: u32 = 0x0b121a;
pub const BORDER: u32 = 0x253447;
pub const BORDER_STRONG: u32 = 0x344963;
pub const TEXT: u32 = 0xdbe5f2;
pub const MUTED: u32 = 0x7c8ea6;
pub const SUBTLE: u32 = 0x64748b;
pub const ACCENT: u32 = 0x38bdf8;
pub const ACCENT_STRONG: u32 = 0x0ea5e9;
pub const DANGER: u32 = 0xf87171;
pub const SUCCESS: u32 = 0x4ade80;
pub const INK_ON_ACCENT: u32 = 0x03121d;
pub const DEFAULT_CONNECTION_COLOR: u32 = 0x38bdf8;

pub const SIDEBAR_WIDTH: f32 = 280.0;
pub const TOPBAR_HEIGHT: f32 = 68.0;
pub const TAB_STRIP_HEIGHT: f32 = 38.0;
pub const FONT_SIZE: f32 = 13.0;
pub const FONT_SIZE_SMALL: f32 = 11.0;
pub const FONT_SIZE_EYEBROW: f32 = 9.0;
pub const FONT_SIZE_TITLE: f32 = 15.0;

/// The embedded UI family, or the system fallback when the font was not
/// fetched with `npm run fonts`.
pub const SATOSHI_FAMILY: &str = "Satoshi Variable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Ui,
    UiBold,
    Small,
    Eyebrow,
    Title,
    Mono,
}

impl Font {
    pub fn size(self) -> f32 {
        match self {
            Self::Ui | Self::UiBold | Self::Mono => FONT_SIZE,
            Self::Small => FONT_SIZE_SMALL,
            Self::Eyebrow => FONT_SIZE_EYEBROW,
            Self::Title => FONT_SIZE_TITLE,
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            Self::Mono => "Consolas",
            _ => SATOSHI_FAMILY,
        }
    }

    pub fn bold(self) -> bool {
        matches!(self, Self::UiBold | Self::Title | Self::Eyebrow)
    }
}

/// Converts `0xRRGGBB` into a Direct2D color.
pub fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: alpha,
    }
}

/// Parses a profile color (`#rrggbb`), falling back to the DBM accent.
pub fn parse_hex(value: &str) -> u32 {
    let trimmed = value.trim();
    if trimmed.len() == 7 && trimmed.starts_with('#') {
        if let Ok(rgb) = u32::from_str_radix(&trimmed[1..], 16) {
            return rgb;
        }
    }
    DEFAULT_CONNECTION_COLOR
}
