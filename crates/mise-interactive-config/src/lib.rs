//! Interactive TOML config editor for mise
//!
//! This crate provides an interactive editor where the TOML config file itself IS the menu.
//! Users navigate and edit the actual TOML structure directly.

mod cursor;
mod document;
mod editor;
mod inline_edit;
mod picker;
mod providers;
mod render;
pub mod schema;

pub use editor::{ConfigResult, InteractiveConfig};
pub use picker::{PickerItem, PickerState};
pub use providers::{
    BackendInfo, BackendProvider, EmptyBackendProvider, EmptySettingProvider, EmptyToolProvider,
    EmptyVersionProvider, SettingInfo, SettingProvider, SettingType, ToolInfo, ToolProvider,
    VERSION_CUSTOM_MARKER, VersionProvider, version_variants,
};
