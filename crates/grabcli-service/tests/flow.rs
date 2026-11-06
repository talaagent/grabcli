//! The fake backend end to end: search → menu → cart → quote → order →
//! track, the text rendering, and phone logins through `Logins`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use grabcli_service::fake::{FAKE_CODE, Fake};
use grabcli_service::{
    CartOp, GrabBackend, GrabError, LoggedIn, LoginError, Logins, MemorySlot, OnDone, Secret,
    SessionSlot, Timing, View, money, text,
};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("rt")
}

#[test]
fn money_groups_and_signs() {
    assert_eq!(money(123_450, "PHP"), "₱1,234.50");
    assert_eq!(money(-5, ""), "-₱0.05");
    assert_eq!(money(100_000_000, "USD"), "USD 1,000,000.00");
}

#[test]
fn the_fake_orders_once_against_its_quote() {
    let fake = Fake::default();
    let slot = Arc::new(MemorySlot::new("acct-1", Some(Fake::session("Sample"))));
    let c = fake.client(Arc::clone(&slot) as Arc<dyn SessionSlot>);
    rt().block_on(async {
        let ms = c.search("chicken", 5).await.expect("search");
        assert_eq!(ms.len(), 1);
        assert!(text::merchants(&ms).contains("`fake-chicken`"));
        let m = c.menu("fake-chicken").await.expect("menu");
        assert!(text::menu(&m, Some("spag")).contains("Sweet Spaghetti"));
        assert!(!text::menu(&m, Some("spag")).contains("Fried Chicken"));
        for _ in 0..2 {
            c.cart(&CartOp::Add {
                merchant_id: "fake-chicken".into(),
                item_id: "fake-chicken:c1".into(),
                quantity: 1,
                note: Some("extra gravy".into()),
            })
            .await
            .expect("add");
        }
        let cart = c.cart(&CartOp::Show).await.expect("show");
        assert_eq!(cart.subtotal, 19_800);
        assert!(text::cart(&cart).contains("(extra gravy)"));
        let other = c
            .cart(&CartOp::Add {
                merchant_id: "fake-ramen".into(),
                item_id: "fake-ramen:og".into(),
                quantity: 1,
                note: None,
            })
            .await;
        assert!(matches!(other, Err(GrabError::Failed(_))));
        let q = c.quote().await.expect("quote");
        assert_eq!(q.total, 19_800 + 4_900 + 500);
        let t = text::quote(&q);
        assert!(t.contains(&q.quote_id) && t.contains("Total ₱252.00"));
        let p = c.place_order(&q.quote_id).await.expect("order");
        assert_eq!(p.order_id, "fake-order-1");
        assert_eq!(*fake.orders.lock().unwrap(), vec![q.quote_id.clone()]);
        assert!(c.cart(&CartOp::Show).await.expect("show").lines.is_empty());
        let tr = c.track(Some("fake-order-1")).await.expect("track");
        assert!(text::tracked(&tr).contains("`fake-order-1`"));
        let h = c.refresh(true).await.expect("refresh");
        assert_eq!(h.display_name.as_deref(), Some("Sample"));
        assert_eq!(slot.session().expect("session")["n"], 1);
    });
}

#[test]
fn a_dead_or_missing_session_needs_a_relogin() {
    let fake = Fake::default();
    let dead = fake.client(Arc::new(MemorySlot::new(
        "a",
        Some(Fake::dead(Fake::session("Sample"))),
    )));
    let none = fake.client(Arc::new(MemorySlot::new("b", None)));
    rt().block_on(async {
        assert!(matches!(
            dead.search("", 1).await,
            Err(GrabError::NeedsRelogin(_))
        ));
        assert!(matches!(
            none.quote().await,
            Err(GrabError::NeedsRelogin(_))
        ));
    });
}

fn fast() -> Timing {
    Timing {
        approval_wait: Duration::from_millis(10),
        send_cooldown_s: 0,
        ..Timing::default()
    }
}

/// Keeps what the logins hand back.
fn sink() -> (Arc<Mutex<Vec<LoggedIn>>>, OnDone) {
    let got: Arc<Mutex<Vec<LoggedIn>>> = Arc::default();
    let g = Arc::clone(&got);
    let done: OnDone = Arc::new(move |l| {
        g.lock().unwrap().push(l);
        Ok(())
    });
    (got, done)
}

fn until(logins: &Logins, person: &str, org: &str, id: &str, f: impl Fn(&View) -> bool) -> View {
    for _ in 0..400 {
        let s = logins.get(person, org, id).expect("get");
        if f(&s.view) {
            return s.view;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the login never got there");
}

#[test]
fn a_code_login_connects_and_is_bound_to_its_person() {
    let logins = Arc::new(Logins::new(
        fast(),
        Arc::new(|| {
            i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
        }),
    ));
    let fake = Fake::default();
    let (got, done) = sink();
    let s = logins
        .start(
            "p1",
            "acct-1",
            Secret::new("+10000000000"),
            fake.login("", None).expect("driver"),
            done,
        )
        .expect("start");
    let v = until(&logins, "p1", "acct-1", &s.id, |v| {
        matches!(v, View::Code { .. })
    });
    assert_eq!(v.state(), "code");
    assert!(matches!(
        logins.get("p2", "acct-1", &s.id),
        Err(LoginError::NotFound(_))
    ));
    assert!(matches!(
        logins.submit("p1", "acct-1", &s.id, Secret::new("12 34")),
        Err(LoginError::Invalid(_))
    ));
    logins
        .submit("p1", "acct-1", &s.id, Secret::new(FAKE_CODE))
        .expect("submit");
    let v = until(&logins, "p1", "acct-1", &s.id, View::finished);
    assert_eq!(
        v,
        View::Done {
            display_name: "Fake Grab".into()
        }
    );
    let got = got.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].org, "acct-1");
    assert_eq!(got[0].session["fake"], true);
}

#[test]
fn approval_step_ups_bad_phones_and_the_send_budget() {
    let logins = Arc::new(Logins::new(
        Timing {
            sends_per_hour: 2,
            ..fast()
        },
        Arc::new(|| 1_000_000),
    ));
    let fake = Fake::default();
    let start = |org: &str, phone: &str| {
        let (_, done) = sink();
        logins.start(
            "p1",
            org,
            Secret::new(phone),
            fake.login("", None).expect("driver"),
            done,
        )
    };
    assert!(matches!(start("a", "12345"), Err(LoginError::Invalid(_))));
    // Ending in 1: approved in the app after two waits.
    let s = start("a", "+10000000001").expect("start");
    let v = until(&logins, "p1", "a", &s.id, View::finished);
    assert_eq!(v.state(), "done");
    // Ending in 9: a step-up the fake can't pass.
    let s = start("a", "+10000000009").expect("start");
    let v = until(&logins, "p1", "a", &s.id, View::finished);
    assert_eq!(v.state(), "failed");
    // Two sends an hour for org `a`; `b` has its own.
    assert!(matches!(
        start("a", "+10000000000"),
        Err(LoginError::Budget(_))
    ));
    let s = start("b", "+10000000000").expect("start");
    until(&logins, "p1", "b", &s.id, |v| {
        matches!(v, View::Code { .. })
    });
    assert!(matches!(
        start("b", "+10000000000"),
        Err(LoginError::State(_))
    ));
    logins.cancel("p1", "b", &s.id).expect("cancel");
    let v = until(&logins, "p1", "b", &s.id, View::finished);
    assert_eq!(v, View::Cancelled);
}
