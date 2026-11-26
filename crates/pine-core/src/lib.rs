//! Pine core: StreamElements and Twitch clients with no GUI dependencies,
//! so they can be unit tested and reused by any front end.

pub mod activity;
pub mod error;
pub mod public;
pub mod se;
pub mod sylva;
pub mod twitch;

pub use activity::{Activity, ActivityKind};
pub use error::{Error, Result};

#[cfg(test)]
pub(crate) mod testutil;
