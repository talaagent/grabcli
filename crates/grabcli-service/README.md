# grabcli-service

A service-shaped Grab (food delivery) client over
[grabcli](../../README.md)'s library crates, for a server or a bot that keeps
many people's Grab accounts in its own storage.

The `grab` binary keeps one account in files; this crate keeps none. It
gives an embedder:

- `api`: plain, serde-able types (`Merchant`, `Menu`, `Cart`, `Quote`,
  `Placed`, `Tracked`), the per-account `GrabClient` (search, menu, cart,
  quote, place order, track, refresh), the `LoginDriver`, the `GrabBackend`
  that makes both, and `money` (`₱1,234.50`).
- `SessionSlot`: the seam to your storage. A session is opaque JSON
  (grabcli's `Session`), read and saved only through the slot; the client's
  silent refreshes save there too. `MemorySlot` is an in-memory one.
- `real`: the backend over `grabcli-core` and `grabfood`.
- `fake`: a backend with no network and made-up merchants, for tests and
  local development (a phone ending in `1` is approved in the app, one
  ending in `9` fails a step-up, any other gets the code `123456`).
- `login`: many phone logins at once (`Logins`), each on its own thread,
  bound to the person who started it, one at a time per account, with a
  per-account code-send budget (60 s apart, 3 an hour by default).
- `text`: the answers as plain text, for a chat reply or a model.

Nothing is printed or read from disk or the environment; diagnostics go to an
optional `Log` hook, and phones and codes stay in `Secret`s that never print.

## Example

```rust,no_run
use std::sync::Arc;
use grabcli_service::{CartOp, GrabBackend, MemorySlot, fake::Fake, text};

# async fn run() -> grabcli_service::GrabResult<()> {
let client = Fake::default().client(Arc::new(MemorySlot::new(
    "acct-1",
    Some(Fake::session("Sample")),
)));
let menu = client.menu("fake-ramen").await?;
client
    .cart(&CartOp::Add {
        merchant_id: menu.merchant.id.clone(),
        item_id: menu.items[0].id.clone(),
        quantity: 1,
        note: None,
    })
    .await?;
println!("{}", text::quote(&client.quote().await?));
# Ok(())
# }
```

See [`examples/fake_order.rs`](examples/fake_order.rs)
(`cargo run -p grabcli-service --example fake_order`).

## Safety

The real backend keeps grabcli's gates: orders go through only with
`allow_ordering = true` in the `grabcli_core::Config` passed to `Real::new`,
and only against a quote id that matches a fresh re-quote. Never retry
`place_order` (Grab's submit has no idempotency key); check `track` instead.
The quote and order path never reads the browse cache.

See the root README's [disclaimer](../../README.md#disclaimer).

## License

[Apache-2.0](../../LICENSE.txt)
