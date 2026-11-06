//! A whole order on the fake backend, no network:
//! `cargo run -p grabcli-service --example fake_order`.
//!
//! Swap `Fake::default()` for `grabcli_service::real::Real::new(config)` and
//! a slot over your own storage for the real thing.

use std::sync::Arc;

use grabcli_service::fake::Fake;
use grabcli_service::{CartOp, GrabBackend, GrabResult, MemorySlot, text};

#[tokio::main(flavor = "current_thread")]
async fn main() -> GrabResult<()> {
    let backend = Fake::default();
    let slot = Arc::new(MemorySlot::new("acct-1", Some(Fake::session("Sample"))));
    let client = backend.client(slot);

    let found = client.search("ramen", 5).await?;
    println!("{}\n", text::merchants(&found));

    let menu = client.menu("fake-ramen").await?;
    println!("{}\n", text::menu(&menu, None));

    let cart = client
        .cart(&CartOp::Add {
            merchant_id: menu.merchant.id.clone(),
            item_id: menu.items[0].id.clone(),
            quantity: 2,
            note: Some("less salt".into()),
        })
        .await?;
    println!("{}\n", text::cart(&cart));

    let quote = client.quote().await?;
    println!("{}\n", text::quote(&quote));

    // Only once the person agreed to this quote id; never retried.
    let placed = client.place_order(&quote.quote_id).await?;
    println!("placed {}\n", placed.order_id);
    println!("{}", text::tracked(&client.track(None).await?));
    Ok(())
}
