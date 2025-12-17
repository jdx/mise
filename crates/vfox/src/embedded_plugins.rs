// This module provides access to embedded vfox plugin Lua code.
// The actual code is generated at build time by build.rs

include!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));
