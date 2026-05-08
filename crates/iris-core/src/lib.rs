//! Cross-cutting domain types and errors for Iris.

pub mod error;
pub mod ids;
pub mod search;
pub mod user;

pub use error::{Error, Result};
