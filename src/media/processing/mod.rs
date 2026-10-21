//! Bounded media inspection, immutable candidates and fenced publication.

mod inspection;
pub mod lifecycle;
pub mod repository;

pub use inspection::{InspectedMedia, inspect, process};
