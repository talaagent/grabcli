//! A service-shaped Grab (food delivery) client over grabcli's library
//! crates, for a server or a bot that keeps many people's Grab accounts.
//!
//! grabcli's `grab` binary keeps one account in files; a service keeps many,
//! in its own storage. This crate is the part such a service needs that is
//! not specific to it:
//!
//! - [`api`]: plain, serde-able types ([`Merchant`], [`Menu`], [`Cart`],
//!   [`Quote`], [`Placed`], [`Tracked`]), the per-account [`GrabClient`],
//!   the [`LoginDriver`] and the [`GrabBackend`] that makes both, and
//!   [`money`]. A session is opaque here (grabcli's serde `Session` as
//!   JSON), read and written only through a [`SessionSlot`] the service
//!   implements over its storage.
//! - [`real`]: the backend over `grabcli-core` and `grabfood`.
//! - [`fake`]: a backend with no network, for tests and local development.
//! - [`login`]: many phone logins at once, each on its own thread, with a
//!   per-account send budget; keyed by plain strings (a `person` who
//!   started it, an `org`: the account it connects).
//! - [`text`]: the answers as plain text (for a chat reply or a model).
//!
//! Nothing here reads files, environment variables or the network on its
//! own, and nothing is printed: diagnostics go to an optional [`Log`] hook.
//! Phone numbers and codes are held in [`Secret`]s and never logged.
//!
//! ```
//! use std::sync::Arc;
//! use grabcli_service::{CartOp, GrabBackend, MemorySlot, fake::Fake, text};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let rt = tokio::runtime::Runtime::new()?;
//! let backend = Fake::default();
//! let slot = Arc::new(MemorySlot::new("acct-1", Some(Fake::session("Sample"))));
//! let client = backend.client(slot);
//! rt.block_on(async {
//!     let found = client.search("ramen", 5).await?;
//!     println!("{}", text::merchants(&found));
//!     let menu = client.menu(&found[0].id).await?;
//!     client
//!         .cart(&CartOp::Add {
//!             merchant_id: menu.merchant.id.clone(),
//!             item_id: menu.items[0].id.clone(),
//!             quantity: 1,
//!             note: None,
//!         })
//!         .await?;
//!     println!("{}", text::quote(&client.quote().await?));
//!     Ok::<_, grabcli_service::GrabError>(())
//! })?;
//! # Ok(())
//! # }
//! ```
//!
//! Placing a real order still needs grabcli's own gate: `allow_ordering` in
//! the [`grabcli_core::Config`] given to [`real::Real::new`], and a quote id
//! that matches a fresh re-quote. Never retry [`GrabClient::place_order`].

#![warn(missing_docs)]

extern crate alloc;

pub mod api;
pub mod fake;
pub mod login;
pub mod real;
pub mod text;
mod util;

use alloc::sync::Arc;
use core::fmt;
use std::sync::Mutex;

use serde_json::Value;

pub use self::api::{
    Cart, CartOp, Fee, GrabBackend, GrabClient, GrabError, GrabResult, Health, Line, LoginDriver,
    Menu, MenuItem, Merchant, Op, Placed, Quote, SessionSlot, Step, Tracked, money,
};
pub use self::login::{LoggedIn, LoginError, Logins, OnDone, Snapshot, Timing, View};

/// A diagnostics hook (a login abandoned, a cart that couldn't be emptied):
/// lines without secrets, for the embedder's log. None: dropped.
pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

/// A secret string (a phone number, a login code): `Debug` never shows it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap `s`.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The value, for Grab's form; never for a log.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `text` with the value replaced (an error body that echoes it).
    pub fn redact(&self, text: &str) -> String {
        if self.0.is_empty() {
            text.to_owned()
        } else {
            text.replace(&self.0, "<redacted>")
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// A [`SessionSlot`] in memory (tests, examples, a single-process tool).
#[derive(Debug, Default)]
pub struct MemorySlot {
    id: String,
    session: Mutex<Option<Value>>,
    cart: Mutex<Option<Value>>,
}

impl MemorySlot {
    /// A slot for account `id` holding `session`.
    pub fn new(id: impl Into<String>, session: Option<Value>) -> Self {
        Self {
            id: id.into(),
            session: Mutex::new(session),
            cart: Mutex::default(),
        }
    }

    /// The session now (as the backend last saved it).
    pub fn session(&self) -> Option<Value> {
        util::lock(&self.session).clone()
    }

    /// The kept cart now.
    pub fn cart(&self) -> Option<Value> {
        util::lock(&self.cart).clone()
    }
}

#[async_trait::async_trait]
impl SessionSlot for MemorySlot {
    fn id(&self) -> &str {
        &self.id
    }

    async fn load(&self) -> GrabResult<Option<Value>> {
        Ok(self.session())
    }

    async fn save(&self, session: &Value) -> GrabResult<()> {
        *util::lock(&self.session) = Some(session.clone());
        Ok(())
    }

    async fn load_cart(&self) -> GrabResult<Option<Value>> {
        Ok(self.cart())
    }

    async fn save_cart(&self, cart: Option<&Value>) -> GrabResult<()> {
        *util::lock(&self.cart) = cart.cloned();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_debugs() {
        let s = Secret::new("+10000000000");
        assert_eq!(format!("{s:?}"), "<redacted>");
        assert_eq!(s.redact("got +10000000000"), "got <redacted>");
        assert!(Secret::default().is_empty());
    }
}
