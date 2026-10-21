//! Group membership and invitation workflows share aggregate locks and role policy.

pub mod http;
mod invitations;
mod membership;
pub(crate) mod repository;

pub use invitations::{accept, invite, revoke_invite};
pub use membership::{add_members, create, members, remove, rename, set_role, transfer};
