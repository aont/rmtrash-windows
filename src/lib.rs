//! Native Recycle Bin discovery, file trashing, and metadata handling.
pub mod codec;
pub mod listing;
pub mod model;
pub mod platform;
pub mod presentation;

#[cfg(any(windows, test))]
mod transaction;
