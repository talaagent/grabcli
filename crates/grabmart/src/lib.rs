//! GrabMart: the mart API client, models, cart and quote parsing (see
//! docs/mart.md).
//!
//! Browsing uses the mart `public` API with reCAPTCHA tokens minted in the CDP
//! browser ([`recaptcha`]); results are cached under namespace `"mart"`. The
//! cart is a value the caller keeps; quotes hit `protected/cart` with the
//! logged-in session; the order path is gated exactly like GrabFood's
//! ([`order`]). Build a [`client::Mart`] from a [`grabcli_core::Context`].
//!
//! # Example: keep a cart in memory
//!
//! ```
//! use grabmart::{Cart, CartItem};
//!
//! let mut cart = Cart::default();
//! cart.add("store-id", "Example store", CartItem {
//!     item_id: "item-id".into(),
//!     quantity: 2,
//!     name: "Example item".into(),
//!     unit_price_minor: 5_000,
//!     campaign_id: None,
//!     comment: String::new(),
//! })?;
//! assert!(!cart.is_empty());
//! # Ok::<(), grabcli_core::Error>(())
//! ```
//!
//! This example makes no network calls. A cart price is only a display snapshot;
//! obtain a live quote through [`Mart`] before considering an order.

pub mod api;
pub mod cart;
pub mod client;
pub mod model;
pub mod order;
pub mod quote;
pub mod recaptcha;

pub use cart::{Cart, CartItem};
pub use client::{Mart, MemoryPacer, Pacer};

/// Package README examples, compiled as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
