//! Application modules and the public composition facade.

pub mod app;
pub mod auth;
pub mod conversations;
pub mod error;
pub mod infrastructure;
pub mod realtime;
pub mod types;
pub mod users;

pub use app::{AppState, build_router};
