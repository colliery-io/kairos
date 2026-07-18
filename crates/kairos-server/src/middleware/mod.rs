//! The tower middleware stack (KAIROS-T-0017): [`auth`] validates OIDC
//! bearer tokens and JIT-provisions users (KAIROS-A-0010); [`tenant`]
//! resolves the organization and enforces membership (KAIROS-A-0005 §2).
//! Auth runs first; tenant consumes its [`auth::AuthContext`].

pub mod auth;
pub mod tenant;
