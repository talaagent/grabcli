//! What a service asks of Grab: the per-account client ([`GrabClient`]:
//! search, menu, cart, quote, order, tracking, the refresh and the liveness
//! check), the login driver ([`LoginDriver`]) and the backend that makes
//! both ([`GrabBackend`]: grabcli's library, or the fake). The session
//! itself is opaque here (grabcli's serde `Session` as JSON), read and
//! written only through a [`SessionSlot`]: wherever the service keeps it.

use alloc::sync::Arc;
use core::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Secret;

/// A cart change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CartOp {
    /// The cart as it is.
    Show,
    /// Add `quantity` of an item (the cart holds one merchant at a time).
    Add {
        /// The merchant's id (from a search).
        merchant_id: String,
        /// The item's id (from the merchant's menu).
        item_id: String,
        /// How many.
        quantity: u32,
        /// A note for the merchant ("no onions").
        note: Option<String>,
    },
    /// Drop an item's line.
    Remove {
        /// The item's id.
        item_id: String,
    },
    /// Empty it.
    Clear,
}

/// One call a service's caller (a tool, a command) asks for, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Restaurants matching `query`.
    Search {
        /// What to look for.
        query: String,
        /// At most this many.
        limit: usize,
    },
    /// A merchant's menu, optionally only the items matching `query`.
    Menu {
        /// The merchant's id.
        merchant_id: String,
        /// A filter on the items' name, category and description.
        query: Option<String>,
    },
    /// A cart change.
    Cart(CartOp),
    /// Price the cart now.
    Quote,
    /// Ongoing orders (or one).
    Track {
        /// One order (none: the ongoing ones).
        order_id: Option<String>,
    },
    /// Place the cart's order against a fresh matching quote.
    PlaceOrder {
        /// The quote id the person agreed to.
        quote_id: String,
    },
}

/// Why a Grab call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrabError {
    /// The session is dead (its SSO refused): the account logs in again.
    #[error("Grab needs re-login: {0}")]
    NeedsRelogin(String),
    /// Anything else (Grab's answer, the network, a bad argument).
    #[error("{0}")]
    Failed(String),
}

/// A Grab call's result.
pub type GrabResult<T> = Result<T, GrabError>;

/// A restaurant from a search.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Merchant {
    /// Grab's merchant id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its cuisines, comma-separated.
    #[serde(default)]
    pub cuisine: String,
    /// Delivery estimate, minutes.
    pub eta_min: Option<u32>,
    /// How far, km.
    pub distance_km: Option<f64>,
    /// Its rating (out of 5).
    pub rating: Option<f64>,
    /// A promo line ("20% off"), if any.
    #[serde(default)]
    pub promo: String,
    /// Whether it takes orders now.
    #[serde(default)]
    pub open: bool,
}

/// One menu item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MenuItem {
    /// The item's id (for [`CartOp::Add`]).
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its menu section.
    #[serde(default)]
    pub category: String,
    /// Minor units (centavos).
    pub price: i64,
    /// Its description.
    #[serde(default)]
    pub description: String,
    /// Whether it can be ordered now.
    #[serde(default)]
    pub available: bool,
}

/// A merchant's menu.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Menu {
    /// Whose menu.
    pub merchant: Merchant,
    /// ISO code of the prices (`PHP`).
    pub currency: String,
    /// The items.
    pub items: Vec<MenuItem>,
}

/// A line of the cart or a quote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The item's id.
    pub item_id: String,
    /// The item's name.
    pub name: String,
    /// How many.
    pub quantity: u32,
    /// The line's total, minor units.
    pub amount: i64,
    /// The note for the merchant.
    #[serde(default)]
    pub note: String,
}

/// The cart (one merchant at a time).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cart {
    /// The merchant's id (empty: an empty cart).
    pub merchant_id: String,
    /// The merchant's name.
    pub merchant_name: String,
    /// ISO code (`PHP`).
    pub currency: String,
    /// The lines.
    pub lines: Vec<Line>,
    /// The lines' sum, minor units.
    pub subtotal: i64,
}

/// A fee of a quote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fee {
    /// What it is ("Delivery").
    pub label: String,
    /// Minor units.
    pub amount: i64,
}

/// A priced checkout of the cart: what an order would cost now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// Grab's id for this exact checkout (items, fees, voucher, address):
    /// an order is placed only against a fresh re-quote with the same id.
    pub quote_id: String,
    /// The merchant's name.
    pub merchant_name: String,
    /// ISO code (`PHP`).
    pub currency: String,
    /// The lines.
    pub lines: Vec<Line>,
    /// Before fees and discounts, minor units.
    pub subtotal: i64,
    /// Delivery and other fees.
    pub fees: Vec<Fee>,
    /// Off the total (positive), with the voucher that gives it.
    pub discount: i64,
    /// The voucher(s) behind the discount.
    #[serde(default)]
    pub voucher: String,
    /// What would be paid, minor units.
    pub total: i64,
    /// Delivery estimate, minutes.
    pub eta_min: Option<u32>,
    /// How it would be paid (Grab's payment type).
    pub payment_method: String,
    /// Where it goes, as a label only ("Home").
    pub address_label: String,
}

/// A placed order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// Grab's order id (for [`GrabClient::track`]).
    pub order_id: String,
    /// Delivery estimate, minutes.
    pub eta_min: Option<u32>,
}

/// An order being delivered (or just delivered).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tracked {
    /// Grab's order id.
    pub order_id: String,
    /// The merchant's name.
    pub merchant_name: String,
    /// Where it is, as Grab words it.
    pub status: String,
    /// Arrival estimate, minutes.
    pub eta_min: Option<u32>,
    /// The driver's name, once assigned.
    #[serde(default)]
    pub driver: String,
}

/// What a refresh or a liveness check found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    /// The Grab account's name (none: not asked this time).
    pub display_name: Option<String>,
    /// When the SSO login lapses (unix seconds), if known.
    pub sso_expires_at: Option<i64>,
}

/// An account's session, wherever the service keeps it (sealed, ideally):
/// read and written only through this (the client's refreshes save here).
#[async_trait::async_trait]
pub trait SessionSlot: Send + Sync {
    /// Whose (the account's key): keys what the backend keeps per account.
    fn id(&self) -> &str;
    /// The session (none: not connected).
    async fn load(&self) -> GrabResult<Option<Value>>;
    /// Keep the session (a refresh renewed it).
    async fn save(&self, session: &Value) -> GrabResult<()>;
    /// The backend's cart, kept beside the session (none: empty).
    async fn load_cart(&self) -> GrabResult<Option<Value>>;
    /// Keep (none: drop) the cart.
    async fn save_cart(&self, cart: Option<&Value>) -> GrabResult<()>;
}

/// One account's Grab, over its [`SessionSlot`].
#[async_trait::async_trait]
pub trait GrabClient: Send + Sync {
    /// Restaurants matching `query` (at most `limit`).
    async fn search(&self, query: &str, limit: usize) -> GrabResult<Vec<Merchant>>;
    /// A merchant's menu.
    async fn menu(&self, merchant_id: &str) -> GrabResult<Menu>;
    /// Change (or show) the cart; the cart after.
    async fn cart(&self, op: &CartOp) -> GrabResult<Cart>;
    /// The cart priced now, with the best voucher that applies.
    async fn quote(&self) -> GrabResult<Quote>;
    /// Place the cart's order against `quote_id` (the caller re-quoted
    /// and matched it). Never retried.
    async fn place_order(&self, quote_id: &str) -> GrabResult<Placed>;
    /// One order's tracking, or the ongoing orders'.
    async fn track(&self, order_id: Option<&str>) -> GrabResult<Vec<Tracked>>;
    /// A silent SSO refresh when the web session is near its end (saved
    /// through the slot), and with `liveness` a check that the account
    /// answers (its name).
    async fn refresh(&self, liveness: bool) -> GrabResult<Health>;
}

/// Where a login is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Grab sent a code (`channel`: sms, whatsapp, ...).
    AwaitingCode {
        /// How it was sent.
        channel: String,
    },
    /// Grab asked the app to approve it.
    AwaitingApproval,
    /// Logged in: the session to keep, the account's name.
    Done {
        /// grabcli's `Session` as JSON.
        session: Value,
        /// How the account is shown.
        display_name: String,
        /// When the SSO login lapses (unix seconds), if known.
        sso_expires_at: Option<i64>,
    },
    /// It ended (a wrong code, a step-up the app can't do: reCAPTCHA,
    /// selfie, PIN), why.
    Failed(String),
}

/// One login in a fresh isolated browser context, driven from its own
/// thread. The phone and the code are passed in and never kept by the
/// caller.
pub trait LoginDriver: Send {
    /// Begin with `phone` (international form).
    fn start(&mut self, phone: &Secret) -> Step;
    /// The code Grab sent.
    fn submit_code(&mut self, code: &Secret) -> Step;
    /// Wait up to `timeout` for the app's approval.
    fn wait(&mut self, timeout: Duration) -> Step;
    /// Stop and dispose of the browser context (also on drop).
    fn cancel(&mut self);
}

/// Makes the clients and the login drivers.
pub trait GrabBackend: Send + Sync {
    /// The client of the account behind `session`.
    fn client(&self, session: Arc<dyn SessionSlot>) -> Arc<dyn GrabClient>;
    /// A login driver on the browser at `cdp_url`; `previous` is the
    /// account's session if it has one (its delivery address carries over).
    ///
    /// # Errors
    /// The browser can't be reached.
    fn login(&self, cdp_url: &str, previous: Option<Value>)
    -> Result<Box<dyn LoginDriver>, String>;
}

/// `₱1,234.50` for `minor` centavos of `currency` (PHP's sign; others by
/// code).
pub fn money(minor: i64, currency: &str) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let m = minor.unsigned_abs();
    let whole = (m / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let cents = m % 100;
    let unit = match currency {
        "" | "PHP" => "₱".to_owned(),
        c => format!("{c} "),
    };
    format!("{sign}{unit}{grouped}.{cents:02}")
}
