#[cfg(feature = "self_update")]
pub mod github;

#[cfg(feature = "self_update")]
pub use github::SelfUpdate;

#[cfg(not(feature = "self_update"))]
pub mod other;

#[cfg(not(feature = "self_update"))]
pub use other::SelfUpdate;
