//! A Grab with no network, for tests and local development. Nothing here
//! reaches Grab; every merchant, item and order is made up.
//!
//! - Login: a phone ending in `1` is approved in the app (after two
//!   waits); one ending in `9` hits a reCAPTCHA step-up (failed); any other
//!   gets a code, `123456`.
//! - The session is `{"fake": true, "account": "<name>", "n": <refreshes>}`;
//!   `"dead": true` in it makes every call need a re-login.
//! - Two merchants, a cart in memory per session account, a quote id that
//!   is a hash of the cart, orders that only count up.
//!
//! [`Fake::session`] makes a connected session without a login.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::time::Duration;
use std::sync::Mutex;

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::api::{
    Cart, CartOp, Fee, GrabBackend, GrabClient, GrabError, GrabResult, Health, Line, LoginDriver,
    Menu, MenuItem, Merchant, Placed, Quote, SessionSlot, Step, Tracked,
};
use crate::Secret;

/// The fake's code.
pub const FAKE_CODE: &str = "123456";

/// The fake backend; its orders are counted (the tests check that an
/// order went through once, or never).
#[derive(Default)]
pub struct Fake {
    carts: Arc<Mutex<BTreeMap<String, Cart>>>,
    /// The quote ids of the orders placed, in order.
    pub orders: Arc<Mutex<Vec<String>>>,
    /// Quote ids of the next quotes, instead of the cart's hash (a test's
    /// price change).
    pub next_quotes: Arc<Mutex<Vec<String>>>,
}

fn merchants() -> Vec<Merchant> {
    vec![
        Merchant {
            id: "fake-chicken".into(),
            name: "Chicken House (Fake)".into(),
            cuisine: "Fast food".into(),
            eta_min: Some(25),
            distance_km: Some(1.2),
            rating: Some(4.7),
            promo: "₱50 off".into(),
            open: true,
        },
        Merchant {
            id: "fake-ramen".into(),
            name: "Ramen Bar (Fake)".into(),
            cuisine: "Japanese".into(),
            eta_min: Some(35),
            distance_km: Some(2.8),
            rating: Some(4.8),
            promo: String::new(),
            open: true,
        },
    ]
}

fn items(merchant: &str) -> Vec<MenuItem> {
    let it = |id: &str, name: &str, price: i64| MenuItem {
        id: format!("{merchant}:{id}"),
        name: name.into(),
        category: "Mains".into(),
        price,
        description: String::new(),
        available: true,
    };
    if merchant == "fake-ramen" {
        vec![
            it("og", "Tonkotsu Ramen", 46_000),
            it("black", "Black Garlic Ramen", 48_000),
        ]
    } else {
        vec![
            it("c1", "Fried Chicken 1pc", 9_900),
            it("spag", "Sweet Spaghetti", 7_500),
        ]
    }
}

struct FakeClient {
    fake: Arc<FakeShared>,
    slot: Arc<dyn SessionSlot>,
}

struct FakeShared {
    carts: Arc<Mutex<BTreeMap<String, Cart>>>,
    orders: Arc<Mutex<Vec<String>>>,
    next_quotes: Arc<Mutex<Vec<String>>>,
}

impl FakeClient {
    /// The session's account, or why not.
    async fn account(&self) -> GrabResult<(String, Value)> {
        let s = self
            .slot
            .load()
            .await?
            .ok_or_else(|| GrabError::NeedsRelogin("not connected".into()))?;
        if s["dead"].as_bool() == Some(true) {
            return Err(GrabError::NeedsRelogin("the SSO login was refused".into()));
        }
        Ok((s["account"].as_str().unwrap_or("fake").to_owned(), s))
    }

    fn cart_of(&self, account: &str) -> Cart {
        crate::util::lock(&self.fake.carts)
            .get(account)
            .cloned()
            .unwrap_or_else(|| Cart {
                currency: "PHP".into(),
                ..Cart::default()
            })
    }

    fn quote_of(&self, cart: &Cart) -> Quote {
        let mut h = Sha256::new();
        h.update(serde_json::to_vec(cart).unwrap_or_default());
        let mut id = format!("fq-{}", &crate::util::hex(&h.finalize())[..12]);
        if let Some(next) = crate::util::lock(&self.fake.next_quotes).pop() {
            id = next;
        }
        let fees = vec![
            Fee {
                label: "Delivery fee".into(),
                amount: 4_900,
            },
            Fee {
                label: "Platform fee".into(),
                amount: 500,
            },
        ];
        let discount = if cart.subtotal >= 20_000 { 5_000 } else { 0 };
        let total = cart.subtotal + fees.iter().map(|f| f.amount).sum::<i64>() - discount;
        Quote {
            quote_id: id,
            merchant_name: cart.merchant_name.clone(),
            currency: "PHP".into(),
            lines: cart.lines.clone(),
            subtotal: cart.subtotal,
            fees,
            discount,
            voucher: if discount > 0 {
                "FAKE50".into()
            } else {
                String::new()
            },
            total,
            eta_min: Some(30),
            payment_method: "GrabPay (fake)".into(),
            address_label: "Home".into(),
        }
    }
}

#[async_trait::async_trait]
impl GrabClient for FakeClient {
    async fn search(&self, query: &str, limit: usize) -> GrabResult<Vec<Merchant>> {
        self.account().await?;
        let q = query.to_lowercase();
        Ok(merchants()
            .into_iter()
            .filter(|m| {
                q.is_empty()
                    || m.name.to_lowercase().contains(&q)
                    || m.cuisine.to_lowercase().contains(&q)
            })
            .take(limit)
            .collect())
    }

    async fn menu(&self, merchant_id: &str) -> GrabResult<Menu> {
        self.account().await?;
        let merchant = merchants()
            .into_iter()
            .find(|m| m.id == merchant_id)
            .ok_or_else(|| GrabError::Failed(format!("no merchant {merchant_id}")))?;
        Ok(Menu {
            items: items(&merchant.id),
            merchant,
            currency: "PHP".into(),
        })
    }

    async fn cart(&self, op: &CartOp) -> GrabResult<Cart> {
        let (account, _) = self.account().await?;
        let mut cart = self.cart_of(&account);
        match op {
            CartOp::Show => {}
            CartOp::Clear => {
                cart = Cart {
                    currency: "PHP".into(),
                    ..Cart::default()
                };
            }
            CartOp::Add {
                merchant_id,
                item_id,
                quantity,
                note,
            } => {
                let m = merchants()
                    .into_iter()
                    .find(|m| m.id == *merchant_id)
                    .ok_or_else(|| GrabError::Failed(format!("no merchant {merchant_id}")))?;
                if !cart.merchant_id.is_empty() && cart.merchant_id != m.id {
                    return Err(GrabError::Failed(
                        "the cart has another merchant's items: clear it first".into(),
                    ));
                }
                let it = items(&m.id)
                    .into_iter()
                    .find(|i| i.id == *item_id)
                    .ok_or_else(|| GrabError::Failed(format!("no item {item_id}")))?;
                cart.merchant_id = m.id;
                cart.merchant_name = m.name;
                cart.lines.push(Line {
                    item_id: it.id,
                    name: it.name,
                    quantity: *quantity,
                    amount: it.price * i64::from(*quantity),
                    note: note.clone().unwrap_or_default(),
                });
            }
            CartOp::Remove { item_id } => {
                let before = cart.lines.len();
                cart.lines.retain(|l| l.item_id != *item_id);
                if cart.lines.len() == before {
                    return Err(GrabError::Failed(format!("{item_id} isn't in the cart")));
                }
                if cart.lines.is_empty() {
                    cart.merchant_id.clear();
                    cart.merchant_name.clear();
                }
            }
        }
        cart.subtotal = cart.lines.iter().map(|l| l.amount).sum();
        crate::util::lock(&self.fake.carts).insert(account, cart.clone());
        Ok(cart)
    }

    async fn quote(&self) -> GrabResult<Quote> {
        let (account, _) = self.account().await?;
        let cart = self.cart_of(&account);
        if cart.lines.is_empty() {
            return Err(GrabError::Failed("the cart is empty".into()));
        }
        Ok(self.quote_of(&cart))
    }

    async fn place_order(&self, quote_id: &str) -> GrabResult<Placed> {
        let (account, _) = self.account().await?;
        let mut orders = crate::util::lock(&self.fake.orders);
        orders.push(quote_id.to_owned());
        crate::util::lock(&self.fake.carts).remove(&account);
        Ok(Placed {
            order_id: format!("fake-order-{}", orders.len()),
            eta_min: Some(30),
        })
    }

    async fn track(&self, order_id: Option<&str>) -> GrabResult<Vec<Tracked>> {
        self.account().await?;
        let n = crate::util::lock(&self.fake.orders).len();
        Ok((1..=n)
            .map(|i| Tracked {
                order_id: format!("fake-order-{i}"),
                merchant_name: "Chicken House (Fake)".into(),
                status: "Driver on the way".into(),
                eta_min: Some(12),
                driver: "Sample Driver (fake)".into(),
            })
            .filter(|t| order_id.is_none_or(|o| o == t.order_id))
            .collect())
    }

    async fn refresh(&self, liveness: bool) -> GrabResult<Health> {
        let (account, mut s) = self.account().await?;
        s["n"] = json!(s["n"].as_i64().unwrap_or(0) + 1);
        self.slot.save(&s).await?;
        Ok(Health {
            display_name: liveness.then_some(account),
            sso_expires_at: s["sso_expires_at"].as_i64(),
        })
    }
}

impl GrabBackend for Fake {
    fn client(&self, session: Arc<dyn SessionSlot>) -> Arc<dyn GrabClient> {
        Arc::new(FakeClient {
            fake: Arc::new(FakeShared {
                carts: Arc::clone(&self.carts),
                orders: Arc::clone(&self.orders),
                next_quotes: Arc::clone(&self.next_quotes),
            }),
            slot: session,
        })
    }

    fn login(
        &self,
        _cdp_url: &str,
        _previous: Option<Value>,
    ) -> Result<Box<dyn LoginDriver>, String> {
        Ok(Box::new(FakeLogin::default()))
    }
}

/// The fake's login (see the module's docs for which phones do what).
#[derive(Default)]
pub struct FakeLogin {
    waits: u32,
}

fn done() -> Step {
    Step::Done {
        session: Fake::session("Fake Grab"),
        display_name: "Fake Grab".into(),
        sso_expires_at: Some(crate::util::now_s() + 60 * 86400),
    }
}

impl Fake {
    /// A connected fake session of `account` (what a fake login yields), its
    /// SSO good for 60 days.
    pub fn session(account: &str) -> Value {
        json!({
            "fake": true,
            "account": account,
            "n": 0,
            "sso_expires_at": crate::util::now_s() + 60 * 86400,
        })
    }

    /// `session`, refused by Grab: every call needs a re-login.
    pub fn dead(mut session: Value) -> Value {
        session["dead"] = json!(true);
        session
    }
}

impl LoginDriver for FakeLogin {
    fn start(&mut self, phone: &Secret) -> Step {
        let p = phone.expose();
        if p.ends_with('9') {
            return Step::Failed("Grab asked for a reCAPTCHA check (a step-up): try later".into());
        }
        if p.ends_with('1') {
            return Step::AwaitingApproval;
        }
        Step::AwaitingCode {
            channel: "sms".into(),
        }
    }

    fn submit_code(&mut self, code: &Secret) -> Step {
        if code.expose() == FAKE_CODE {
            done()
        } else {
            Step::Failed("Grab refused the code".into())
        }
    }

    fn wait(&mut self, timeout: Duration) -> Step {
        self.waits += 1;
        if self.waits >= 2 {
            return done();
        }
        std::thread::sleep(timeout.min(Duration::from_millis(50)));
        Step::AwaitingApproval
    }

    fn cancel(&mut self) {
        self.waits = 0;
    }
}
