//! grabcli's library as the [`GrabBackend`]: `grabfood` for the client,
//! `grabcli_core::login::LoginDriver` for the login.
//!
//! - The session lives wherever the [`SessionSlot`] keeps it: [`SlotStore`]
//!   is grabcli's `SessionStore` over the slot, so every refresh grabcli
//!   makes (SSO, hydra) is saved there. Nothing is read from or written to
//!   grabcli's own config or state directories.
//! - The cart is grabcli's [`grabfood::Cart`] plus the names and unit
//!   prices seen when adding, kept beside the session (unsealed: ids and
//!   prices only).
//! - The response cache is in memory, one per account, for browsing only
//!   (grabfood never reads it on the quote and order path).
//! - Each call builds a fresh `Context` from the stored session.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::time::Duration;
use std::sync::Mutex;

use grabcli_core::cache::{Cache, CacheMode, MemoryCache};
use grabcli_core::error::{Error as CoreError, ErrorKind};
use grabcli_core::login::{
    Channel, LoginDriver as CoreDriver, LoginRequest, LoginState, MemorySendBudget,
};
use grabcli_core::{Context, Session, SessionStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::api::{
    Cart, CartOp, Fee, GrabBackend, GrabClient, GrabError, GrabResult, Health, Line, LoginDriver,
    Menu, MenuItem, Merchant, Placed, Quote, SessionSlot, Step, Tracked,
};
use crate::{Log, Secret};

/// Refresh the web session when it ends within this (seconds).
const REFRESH_AHEAD: i64 = 6 * 3600;
/// Ongoing orders [`GrabClient::track`] looks at without an id.
const TRACK_MAX: u32 = 3;

/// grabcli's error, ours: a dead or missing login needs a re-login.
fn map_err(e: &CoreError) -> GrabError {
    match e.kind {
        ErrorKind::AuthExpired | ErrorKind::AuthMissing => {
            GrabError::NeedsRelogin(e.message.clone())
        }
        _ => GrabError::Failed(e.message.clone()),
    }
}

fn to_core(e: &GrabError) -> CoreError {
    match e {
        GrabError::NeedsRelogin(m) => CoreError::new(ErrorKind::AuthMissing, m.clone()),
        GrabError::Failed(m) => CoreError::new(ErrorKind::Io, m.clone()),
    }
}

/// grabcli's `SessionStore` over an account's slot.
pub struct SlotStore(pub Arc<dyn SessionSlot>);

#[async_trait::async_trait]
impl SessionStore for SlotStore {
    async fn load(&self) -> grabcli_core::Result<Option<Session>> {
        match self.0.load().await.map_err(|e| to_core(&e))? {
            None => Ok(None),
            Some(v) => serde_json::from_value(v)
                .map(Some)
                .map_err(|_| CoreError::parse("the stored Grab session doesn't parse")),
        }
    }

    async fn save(&self, session: &Session) -> grabcli_core::Result<()> {
        let v = serde_json::to_value(session)
            .map_err(|_| CoreError::parse("the Grab session doesn't serialize"))?;
        self.0.save(&v).await.map_err(|e| to_core(&e))
    }
}

/// The kept cart: grabcli's, with what the tools show of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Kept {
    cart: grabfood::Cart,
    #[serde(default)]
    merchant_name: String,
    /// Item id: (name, unit price in centavos).
    #[serde(default)]
    items: BTreeMap<String, (String, i64)>,
}

impl Kept {
    fn view(&self) -> Cart {
        let lines: Vec<Line> = self
            .cart
            .items
            .iter()
            .map(|i| {
                let (name, unit) = self.items.get(&i.item_id).cloned().unwrap_or_default();
                Line {
                    item_id: i.item_id.clone(),
                    name,
                    quantity: u32::try_from(i.quantity).unwrap_or(0),
                    amount: unit.saturating_mul(i.quantity),
                    note: i.comment.clone(),
                }
            })
            .collect();
        Cart {
            merchant_id: self.cart.merchant_id.clone(),
            merchant_name: self.merchant_name.clone(),
            currency: "PHP".into(),
            subtotal: lines.iter().map(|l| l.amount).sum(),
            lines,
        }
    }
}

fn eta(m: Option<i64>) -> Option<u32> {
    m.and_then(|v| u32::try_from(v).ok())
}

fn merchant(m: &grabfood::discover::Merchant) -> Merchant {
    Merchant {
        id: m.id.clone(),
        name: m.name.clone(),
        cuisine: m.cuisines.join(", "),
        eta_min: eta(m.eta_min),
        distance_km: m.distance_km,
        rating: m.rating,
        promo: m.promo.clone().unwrap_or_default(),
        open: m.open.unwrap_or(true),
    }
}

fn menu_of(m: &grabfood::Menu) -> Menu {
    Menu {
        merchant: Merchant {
            id: m.merchant_id.clone(),
            name: m.name.clone(),
            eta_min: eta(m.eta_min),
            open: m.open,
            ..Merchant::default()
        },
        currency: "PHP".into(),
        items: m
            .items
            .iter()
            .map(|i| MenuItem {
                id: i.id.clone(),
                name: i.name.clone(),
                category: i.category.clone(),
                price: i.price.minor(),
                description: i.description.clone(),
                available: i.available,
            })
            .collect(),
    }
}

fn quote_of(q: &grabfood::Quote, kept: &Kept) -> Quote {
    let mut fees: Vec<Fee> = Vec::new();
    if q.delivery_after.minor() != 0 {
        fees.push(Fee {
            label: "Delivery".into(),
            amount: q.delivery_after.minor(),
        });
    }
    fees.extend(q.fees.iter().map(|f| Fee {
        label: f.name.clone(),
        amount: f.amount.minor(),
    }));
    let discount: i64 = q.promos.iter().map(|p| p.amount.minor().abs()).sum();
    Quote {
        quote_id: q.id.clone(),
        merchant_name: q.merchant_name.clone(),
        currency: "PHP".into(),
        lines: kept.view().lines,
        subtotal: q.subtotal.minor(),
        fees,
        discount,
        voucher: q
            .promos
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        total: q.total_after.minor(),
        eta_min: eta(q.eta_min),
        payment_method: q.payment_type_id.clone(),
        // A label only: the street stays with Grab.
        address_label: q
            .address_detail
            .clone()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| "saved address".into()),
    }
}

fn tracked(t: &grabfood::Tracking) -> Tracked {
    Tracked {
        order_id: t.order_id.clone(),
        merchant_name: t.merchant.name.clone().unwrap_or_default(),
        status: t.human_state.clone(),
        eta_min: eta(t.eta.max_minutes.or(t.eta.min_minutes)),
        driver: t
            .driver
            .as_ref()
            .and_then(|d| d.name.clone())
            .unwrap_or_default(),
    }
}

/// grabcli, for every account.
pub struct Real {
    config: grabcli_core::Config,
    caches: Mutex<BTreeMap<String, Arc<MemoryCache>>>,
    log: Option<Log>,
}

impl Real {
    /// grabcli on `config` (its `allow_ordering` gates
    /// [`GrabClient::place_order`]; its `cdp` is the browser hydra is
    /// minted in, when needed).
    pub fn new(config: grabcli_core::Config) -> Self {
        Self {
            config,
            caches: Mutex::default(),
            log: None,
        }
    }

    /// Send diagnostics (no secrets) to `log`.
    #[must_use]
    pub fn with_log(mut self, log: Log) -> Self {
        self.log = Some(log);
        self
    }

    fn cache(&self, org: &str) -> Arc<MemoryCache> {
        Arc::clone(
            crate::util::lock(&self.caches)
                .entry(org.to_owned())
                .or_default(),
        )
    }
}

impl GrabBackend for Real {
    fn client(&self, session: Arc<dyn SessionSlot>) -> Arc<dyn GrabClient> {
        let cache = self.cache(session.id());
        Arc::new(RealClient {
            config: self.config.clone(),
            cache,
            slot: session,
            log: self.log.clone(),
        })
    }

    fn login(
        &self,
        cdp_url: &str,
        previous: Option<Value>,
    ) -> Result<Box<dyn LoginDriver>, String> {
        if cdp_url.is_empty() {
            return Err("no CDP URL: no browser to log in with".into());
        }
        Ok(Box::new(RealLogin {
            cdp: cdp_url.to_owned(),
            previous: previous.and_then(|v| serde_json::from_value(v).ok()),
            inner: None,
        }))
    }
}

struct RealClient {
    config: grabcli_core::Config,
    cache: Arc<MemoryCache>,
    slot: Arc<dyn SessionSlot>,
    log: Option<Log>,
}

impl RealClient {
    async fn ctx(&self, browse: bool) -> GrabResult<Context> {
        let store: Arc<dyn SessionStore> = Arc::new(SlotStore(Arc::clone(&self.slot)));
        let cache: Arc<dyn Cache> = Arc::clone(&self.cache) as Arc<dyn Cache>;
        let mut ctx = grabfood::context(self.config.clone(), store, cache)
            .await
            .map_err(|e| map_err(&e))?;
        if browse {
            ctx.set_cache_mode(CacheMode::Normal);
        }
        Ok(ctx)
    }

    async fn kept(&self) -> GrabResult<Kept> {
        Ok(self
            .slot
            .load_cart()
            .await?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    async fn keep(&self, k: &Kept) -> GrabResult<()> {
        if k.cart.is_empty() {
            return self.slot.save_cart(None).await;
        }
        let v = serde_json::to_value(k).map_err(|e| GrabError::Failed(e.to_string()))?;
        self.slot.save_cart(Some(&v)).await
    }
}

#[async_trait::async_trait]
impl GrabClient for RealClient {
    async fn search(&self, query: &str, limit: usize) -> GrabResult<Vec<Merchant>> {
        let mut ctx = self.ctx(true).await?;
        let opts = grabfood::discover::ListOpts {
            limit: Some(limit),
            pages: 1,
            ..Default::default()
        };
        let l = grabfood::browse::search(&mut ctx, query, &opts, false, false, None)
            .await
            .map_err(|e| map_err(&e))?;
        Ok(l.merchants.iter().take(limit).map(merchant).collect())
    }

    async fn menu(&self, merchant_id: &str) -> GrabResult<Menu> {
        let mut ctx = self.ctx(true).await?;
        let m = grabfood::menu(&mut ctx, merchant_id, grabfood::MenuSource::Cached)
            .await
            .map_err(|e| map_err(&e))?;
        Ok(menu_of(&m))
    }

    async fn cart(&self, op: &CartOp) -> GrabResult<Cart> {
        let mut k = self.kept().await?;
        match op {
            CartOp::Show => return Ok(k.view()),
            CartOp::Clear => k = Kept::default(),
            CartOp::Remove { item_id } => {
                grabfood::cart_remove(&mut k.cart, item_id).map_err(|e| map_err(&e))?;
                if k.cart.is_empty() {
                    k = Kept::default();
                }
            }
            CartOp::Add {
                merchant_id,
                item_id,
                quantity,
                note,
            } => {
                let mut ctx = self.ctx(true).await?;
                grabfood::cart_add(
                    &mut ctx,
                    &mut k.cart,
                    merchant_id,
                    item_id,
                    i64::from(*quantity),
                    &[],
                    note.clone(),
                )
                .await
                .map_err(|e| map_err(&e))?;
                // The same (cached) menu, for the name and the price.
                let m = grabfood::menu(&mut ctx, merchant_id, grabfood::MenuSource::Cached)
                    .await
                    .map_err(|e| map_err(&e))?;
                k.merchant_name.clone_from(&m.name);
                if let Some(i) = m.item(item_id) {
                    k.items
                        .insert(item_id.clone(), (i.name.clone(), i.price.minor()));
                }
            }
        }
        self.keep(&k).await?;
        Ok(k.view())
    }

    async fn quote(&self) -> GrabResult<Quote> {
        let k = self.kept().await?;
        if k.cart.is_empty() {
            return Err(GrabError::Failed("the cart is empty".into()));
        }
        let mut ctx = self.ctx(false).await?;
        let q = grabfood::quote(&mut ctx, &k.cart, None, None)
            .await
            .map_err(|e| map_err(&e))?;
        Ok(quote_of(&q, &k))
    }

    async fn place_order(&self, quote_id: &str) -> GrabResult<Placed> {
        let mut k = self.kept().await?;
        let mut ctx = self.ctx(false).await?;
        // grabfood re-quotes and refuses a different quote id itself too.
        let p = grabfood::place_order(&mut ctx, &mut k.cart, quote_id, None)
            .await
            .map_err(|e| map_err(&e))?;
        if let Err(e) = self.slot.save_cart(None).await
            && let Some(l) = &self.log
        {
            l(&format!("{}: emptying the cart: {e}", self.slot.id()));
        }
        Ok(Placed {
            order_id: p.order_id,
            eta_min: eta(p.quote.eta_min),
        })
    }

    async fn track(&self, order_id: Option<&str>) -> GrabResult<Vec<Tracked>> {
        let mut ctx = self.ctx(false).await?;
        let ids: Vec<String> = match order_id {
            Some(id) => vec![id.to_owned()],
            None => grabfood::orders(&mut ctx, true, TRACK_MAX)
                .await
                .map_err(|e| map_err(&e))?
                .into_iter()
                .map(|o| o.order_id)
                .collect(),
        };
        let mut out = Vec::new();
        for id in ids {
            let t = grabfood::track(&mut ctx, &id)
                .await
                .map_err(|e| map_err(&e))?;
            out.push(tracked(&t));
        }
        Ok(out)
    }

    async fn refresh(&self, liveness: bool) -> GrabResult<Health> {
        let mut ctx = self.ctx(false).await?;
        let now = crate::util::now_s();
        let due = ctx
            .session()
            .gfc_session_exp()
            .is_none_or(|e| e - now < REFRESH_AHEAD);
        if due {
            ctx.refresh_session().await.map_err(|e| map_err(&e))?;
        }
        if liveness {
            // A signed-in call: the ongoing orders (none is fine).
            grabfood::orders(&mut ctx, true, 1)
                .await
                .map_err(|e| map_err(&e))?;
        }
        Ok(Health {
            // grabcli has no account-name call; the name stays as connected.
            display_name: None,
            sso_expires_at: ctx.session().sso_expires_at(),
        })
    }
}

/// A login on grabcli's step driver, from the login's own thread.
struct RealLogin {
    cdp: String,
    previous: Option<Session>,
    inner: Option<CoreDriver>,
}

/// The masked phone (`+1••••••••00`) the account is shown by: grabcli
/// has no account-name call.
fn masked(s: &LoginState) -> Option<String> {
    match s {
        LoginState::AwaitingCode { phone_masked, .. }
        | LoginState::AwaitingApproval { phone_masked, .. } => Some(phone_masked.clone()),
        LoginState::Done(_) | LoginState::Failed(_) => None,
    }
}

impl RealLogin {
    fn step(&mut self, name: Option<String>) -> Step {
        let Some(d) = self.inner.as_mut() else {
            return Step::Failed("the login didn't start".into());
        };
        match d.state() {
            LoginState::AwaitingCode { channel, error, .. } => match error {
                Some(e) => Step::Failed(e.clone()),
                None => Step::AwaitingCode {
                    channel: channel.clone().unwrap_or_default().to_ascii_lowercase(),
                },
            },
            LoginState::AwaitingApproval { .. } => Step::AwaitingApproval,
            LoginState::Failed(m) => Step::Failed(m.clone()),
            LoginState::Done(s) => {
                let sso = s.sso_expires_at();
                match serde_json::to_value(s) {
                    Ok(session) => Step::Done {
                        session,
                        display_name: name.unwrap_or_else(|| "Grab".into()),
                        sso_expires_at: sso,
                    },
                    Err(_) => Step::Failed("the Grab session doesn't serialize".into()),
                }
            }
        }
    }

    fn name(&self) -> Option<String> {
        self.inner.as_ref().and_then(|d| masked(d.state()))
    }
}

impl LoginDriver for RealLogin {
    fn start(&mut self, phone: &Secret) -> Step {
        // `Logins` keeps the per-account send budget; grabcli's own is per
        // login.
        let budget = MemorySendBudget::default();
        let req = LoginRequest {
            cdp_url: &self.cdp,
            phone: phone.expose(),
            channel: Channel::Sms,
            previous: self.previous.clone(),
            copy_browser_address: false,
        };
        self.inner = Some(crate::util::block_on(CoreDriver::start(req, &budget)));
        let n = self.name();
        self.step(n)
    }

    fn submit_code(&mut self, code: &Secret) -> Step {
        let n = self.name();
        if let Some(d) = self.inner.as_mut() {
            crate::util::block_on(d.submit_code(code.expose()));
        }
        self.step(n)
    }

    fn wait(&mut self, timeout: Duration) -> Step {
        let n = self.name();
        if let Some(d) = self.inner.as_mut() {
            crate::util::block_on(d.poll(timeout));
        }
        self.step(n)
    }

    fn cancel(&mut self) {
        if let Some(d) = self.inner.as_mut() {
            crate::util::block_on(d.cancel());
        }
    }
}

impl Drop for RealLogin {
    fn drop(&mut self) {
        if self
            .inner
            .as_ref()
            .is_some_and(|d| !d.state().is_terminal())
        {
            self.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use grabcli_core::money::Money;
    use serde_json::json;

    use super::*;
    use crate::MemorySlot;

    fn slot() -> Arc<MemorySlot> {
        Arc::new(MemorySlot::new("acct-1", Some(json!({}))))
    }

    #[test]
    fn errors_map_to_relogin_or_failed() {
        let e = CoreError::new(ErrorKind::AuthExpired, "401");
        assert_eq!(map_err(&e), GrabError::NeedsRelogin("401".into()));
        let e = CoreError::new(ErrorKind::QuoteChanged, "changed");
        assert_eq!(map_err(&e), GrabError::Failed("changed".into()));
        assert_eq!(
            to_core(&GrabError::NeedsRelogin("x".into())).kind,
            ErrorKind::AuthMissing
        );
    }

    #[test]
    fn the_session_round_trips_through_the_slot() {
        let s = slot();
        let st = SlotStore(Arc::clone(&s) as Arc<dyn SessionSlot>);
        let mut sess = crate::util::block_on(st.load())
            .expect("load")
            .expect("some");
        sess.payment_type_id = Some("CASH".into());
        crate::util::block_on(st.save(&sess)).expect("save");
        assert_eq!(s.session().expect("session")["payment_type_id"], "CASH");
        let back = crate::util::block_on(st.load())
            .expect("load")
            .expect("some");
        assert_eq!(back.payment_type_id.as_deref(), Some("CASH"));
    }

    #[test]
    fn the_cart_is_kept_beside_the_session_without_grab() {
        let s = slot();
        let mut k = Kept {
            merchant_name: "Sample Kitchen".into(),
            ..Kept::default()
        };
        k.cart.merchant_id = "m1".into();
        for (id, q) in [("i1", 2), ("i2", 1)] {
            k.cart.items.push(grabfood::CartItem {
                item_id: id.into(),
                quantity: q,
                modifiers: Vec::new(),
                comment: String::new(),
            });
        }
        k.items
            .insert("i1".into(), ("Fried Chicken".into(), 10_000));
        k.items.insert("i2".into(), ("Spaghetti".into(), 6_500));
        let c = RealClient {
            config: grabcli_core::Config::default(),
            cache: Arc::default(),
            slot: Arc::clone(&s) as Arc<dyn SessionSlot>,
            log: None,
        };
        crate::util::block_on(c.keep(&k)).expect("keep");
        // Show and remove never call Grab.
        let v = crate::util::block_on(c.cart(&CartOp::Show)).expect("show");
        assert_eq!(v.subtotal, 26_500);
        assert_eq!(v.lines[0].name, "Fried Chicken");
        assert_eq!(v.lines[0].amount, 20_000);
        let v = crate::util::block_on(c.cart(&CartOp::Remove {
            item_id: "i1".into(),
        }))
        .expect("remove");
        assert_eq!(v.subtotal, 6_500);
        assert!(s.cart().is_some());
        assert!(s.session().is_some(), "the session stays");
        let v = crate::util::block_on(c.cart(&CartOp::Clear)).expect("clear");
        assert!(v.lines.is_empty());
        assert!(s.cart().is_none());
        assert!(
            crate::util::block_on(c.quote()).is_err(),
            "an empty cart isn't quoted"
        );
    }

    #[test]
    fn a_quote_maps_fees_promos_and_a_label_only() {
        let q = grabfood::Quote {
            id: "q-1".into(),
            merchant_name: "Sample Kitchen".into(),
            subtotal: Money(26_500),
            delivery_after: Money(4_900),
            fees: vec![grabfood::quote::Fee {
                name: "Small order fee".into(),
                amount: Money(1_000),
            }],
            promos: vec![grabfood::quote::Promo {
                name: "SAVE50".into(),
                amount: Money(-5_000),
            }],
            total_after: Money(27_400),
            eta_min: Some(35),
            payment_type_id: "CASH".into(),
            address: "1 Example St".into(),
            address_detail: Some("Home".into()),
            ..grabfood::Quote::default()
        };
        let out = quote_of(&q, &Kept::default());
        assert_eq!(out.quote_id, "q-1");
        assert_eq!(out.fees.len(), 2);
        assert_eq!(out.fees[0].amount, 4_900);
        assert_eq!(out.discount, 5_000);
        assert_eq!(out.voucher, "SAVE50");
        assert_eq!(out.total, 27_400);
        assert_eq!(out.address_label, "Home");
        assert!(!format!("{out:?}").contains("Example St"));
    }

    #[test]
    fn no_browser_no_login() {
        let r = Real::new(grabcli_core::Config::default());
        assert!(r.login("", None).is_err());
        assert!(r.login("http://127.0.0.1:9", None).is_ok());
    }
}
