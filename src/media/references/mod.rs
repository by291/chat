//! Message/avatar binding and authorized downloads.

pub mod repository;
mod service;

pub use service::{avatar_id, bind_avatar, bind_message, download, get_attachment};
