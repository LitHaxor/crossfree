//! crossfree's launcher core.
//!
//! - [`Engine`]: a Wine engine bundle (arm64 Wine + FEX + the graphics
//!   layers), with a check of its signed loader.
//! - [`Prefixes`]: Wine prefixes under crossfree's data directory.
//! - [`Launch`]: starts a Windows program in a prefix, following the
//!   engine's launcher contract for the environment.
//! - [`BottleSettings`]: per-prefix settings (graphics backends, MSync, Metal
//!   HUD), applied to every program in the prefix.
//! - [`GameProfile`]: what crossfree knows about one program, turned into
//!   the engine's per-process rules; it overrides the bottle's settings.
//! - [`shortcuts`]: the programs installed in a prefix (Start Menu), with
//!   their icons extracted into [`icon_cache_dir`].
//! - [`named_engine`]: the engine mirrored so a program shows under its own
//!   name in the Dock, not as "wine".
//!
//! The engine today is mach9's prebuilt bundle; its rules come from
//! `compatdb-core`, pinned to the same release.

mod apps;
mod dock;
mod engine;
mod error;
mod icons;
mod launch;
mod prefix;
mod profile;
mod settings;
#[cfg(test)]
mod test_support;

pub use apps::{AppShortcut, shortcuts};
pub use dock::{loader_cache_dir, named_engine};
pub use engine::{Engine, Verification};
pub use error::Error;
pub use icons::{Artwork, icon_cache_dir};
pub use launch::{Launch, program_folder};
pub use prefix::{Prefixes, validate_name};
pub use profile::{D3d9Backend, DxgiBackend, GameProfile, rules};
pub use settings::{BottleSettings, LOCK_FILE, SETTINGS_FILE};

/// Result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;
