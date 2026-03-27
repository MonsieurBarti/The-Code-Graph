#![forbid(unsafe_code)]

pub mod analysis;
pub mod error;
pub mod model;
pub mod ports;
pub mod traversal;
pub mod use_cases;

pub mod test_support;

pub use error::{CodeGraphError, Result};
