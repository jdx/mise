//! Core identity trait for backends
//!
//! This trait defines the fundamental identity of a backend and is required
//! by all other backend traits.

use std::fmt::Debug;
use std::sync::Arc;

use crate::cli::args::BackendArg;
use crate::plugins::{PluginEnum, PluginType};

use super::super::backend_type::BackendType;

/// Core identity trait for backends.
///
/// This is the foundational trait that all backends must implement.
/// It provides identity, type information, and optional plugin association.
pub trait BackendIdentity: Debug + Send + Sync {
    /// Returns the unique identifier for this backend (short name).
    ///
    /// This is typically the tool's short name like "node", "python", etc.
    fn id(&self) -> &str {
        &self.ba().short
    }

    /// Returns the human-readable tool name.
    fn tool_name(&self) -> String {
        self.ba().tool_name()
    }

    /// Returns the backend type (Core, Asdf, Vfox, Cargo, etc.)
    fn get_type(&self) -> BackendType {
        BackendType::Core
    }

    /// Returns a reference to the BackendArg for this backend.
    ///
    /// This is the only required method - all other methods have default implementations.
    fn ba(&self) -> &Arc<BackendArg>;

    /// Returns the plugin type if this backend is backed by a plugin.
    fn get_plugin_type(&self) -> Option<PluginType> {
        None
    }

    /// Returns the plugin if this backend is backed by one.
    fn plugin(&self) -> Option<&PluginEnum> {
        None
    }
}
