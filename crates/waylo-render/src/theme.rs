//! Colors and measures shared by every piece of Waylo.

/// Background of floating surfaces (translucent dark glass).
pub const SURFACE: u32 = 0x101012d9;
pub const BORDER: u32 = 0xffffff1f;

pub const TEXT: u32 = 0xffffffee;
pub const TEXT_MUTED: u32 = 0xffffff8c;
pub const TEXT_FAINT: u32 = 0xffffff59;
pub const ACCENT: u32 = 0x6cb6ffff;

/// Active workspace: white pill with dark text.
pub const ACTIVE_BG: u32 = 0xfffffff2;
pub const ACTIVE_FG: u32 = 0x111113ff;
/// Workspace with windows: faint capsule.
pub const OCCUPIED_BG: u32 = 0xffffff2e;
/// Background when the pointer is over it.
pub const HOVER_BG: u32 = 0xffffff26;

pub const RADIUS: f32 = 14.;
