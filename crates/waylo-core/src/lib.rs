//! Waylo logic without a user interface.
//!
//! Everything that talks to the system lives here (Hyprland, volume, network, battery, brightness,
//! configuration files) and can be used from any program in the suite: the bar,
//! the Exposé, the UI or the CLI.
//! Rule: this crate depends on no other Waylo crate nor on any drawing or Wayland code.

pub mod apps;
pub mod brightness;
pub mod exec;
pub mod hypr;
pub mod icons;
pub mod layout;
pub mod paths;
pub mod profiles;
pub mod sys;
pub mod workspaces;
