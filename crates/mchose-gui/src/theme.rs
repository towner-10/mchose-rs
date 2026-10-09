//! Zed's default **One Dark** design tokens, taken from
//! `assets/themes/one/one.json` in the Zed repository.
//!
//! Keeping them in one place is what makes the UI match Zed, and re-theming is
//! a single-file change.

/// UI font. Zed ships "Zed Sans"; fall back to an installed humanist sans.
pub const FONT: &str = "Noto Sans";

pub const EDITOR_BG: u32 = 0x282c33; // editor.background / window
pub const SURFACE: u32 = 0x2f343e; // panel.background
pub const ELEMENT: u32 = 0x2e343e; // element.background
pub const ELEMENT_HOVER: u32 = 0x363c46; // element.hover
pub const ELEMENT_ACTIVE: u32 = 0x454a56; // element.active / element.selected
pub const TITLEBAR: u32 = 0x3b414d; // title_bar.background / status_bar.background
pub const BORDER: u32 = 0x464b57; // border
pub const BORDER_VARIANT: u32 = 0x363c46; // border.variant
pub const TEXT: u32 = 0xdce0e5;
pub const TEXT_MUTED: u32 = 0xa9afbc;
pub const TEXT_FAINT: u32 = 0x878a98;
pub const ACCENT: u32 = 0x74ade8; // icon.accent / link_text.hover
pub const ACCENT_DIM: u32 = 0x46618a;
pub const SUCCESS: u32 = 0xa1c181;
pub const WARNING: u32 = 0xdec184;
pub const ERROR: u32 = 0xd07277;
pub const WHITE: u32 = 0xffffff;
