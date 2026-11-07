//! Shared core for grabcli: the parts every Grab service reuses — config,
//! session, auth/hydra, the allowlisted HTTP client, the read-only response
//! cache and money helpers.
//!
//! The library owns no storage and reads no files or environment: an
//! embedder builds a [`Context`] with [`Context::new`]`(config, session_store,
//! cache)`, passing an `Arc<dyn` [`SessionStore`]`>` and an `Arc<dyn`
//! [`Cache`]`>`. The `grab` CLI uses a `session.json` file store and the
//! SQLite cache from `grabcli-sqlite`; a daemon can back both with its own
//! database. Service crates (e.g. `grabfood`) register their URL allowlist
//! and make calls through the context.
//!
//! # Example: format a price
//!
//! ```
//! use grabcli_core::Money;
//!
//! let price = Money(12_345); // integer centavos, not floating-point pesos
//! assert_eq!(price.peso(), "₱123.45");
//! assert_eq!(price.minor(), 12_345);
//! ```
//!
//! For HTTP calls, construct a [`Context`] with your own [`SessionStore`] and
//! [`Cache`]; see `grabfood::context` or `grabcli-service` for service-level setup.

pub mod cache;
pub mod cdp;
pub mod config;
pub mod ctx;
pub mod error;
pub mod http;
pub mod hydra;
pub mod jwt;
pub mod location;
pub mod login;
pub mod money;
pub mod places;
pub mod session;
pub mod sso;
pub mod store;

pub use cache::{Cache, CacheHandle, CacheKey, CacheKind, CacheMode, Fetched};
pub use config::Config;
pub use ctx::Context;
pub use error::{Error, ErrorKind, Result};
pub use http::{Allowlist, Client, Hydra, Request};
pub use location::Location;
pub use money::Money;
pub use session::Session;
pub use store::{MemorySessionStore, SessionStore};

/// Package README examples, compiled as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
