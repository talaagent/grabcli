# grabmart

[![crates.io](https://img.shields.io/crates/v/grabmart.svg)](https://crates.io/crates/grabmart)
[![docs.rs](https://docs.rs/grabmart/badge.svg)](https://docs.rs/grabmart)

GrabMart API client for [grabcli](https://github.com/talaagent/grabcli/blob/master/README.md).

This is an internal building block of the `grab` CLI, not a general-purpose
library. Its API is not stable.

## Modules

- `api`: public and protected endpoint URLs and the URL allowlist; the
  order-submission URL is only registered when ordering is enabled.
- `client`: `Mart::new(ctx)`; requests paced by an injectable `Pacer` (at
  least 2 s apart, one retry on `429`).
- `recaptcha`: the reCAPTCHA v3 token, minted in the browser over CDP and never
  printed or cached.
- `model`: stores, departments and products.
- `cart`, `quote`: the mart cart (a value, separate from food's) and price
  quotes.
- `order`: gated order submission, sharing GrabFood's confirmation gate
  ([docs/mart.md](https://github.com/talaagent/grabcli/blob/master/docs/mart.md)).

## How it fits

Built on [`grabcli-core`](https://docs.rs/grabcli-core) for session, auth (including
GrabMart's own GrabID web login), HTTP and the read-only browse cache, and reuses [`grabfood`](https://docs.rs/grabfood)'s order gate. The
[`grabcli`](https://docs.rs/grabcli) binary implements `grab mart` on top of it.

See the root README's [disclaimer](https://github.com/talaagent/grabcli/blob/master/README.md#disclaimer).

## Example

### keep a cart in memory

```
use grabmart::{Cart, CartItem};

let mut cart = Cart::default();
cart.add("store-id", "Example store", CartItem {
    item_id: "item-id".into(),
    quantity: 2,
    name: "Example item".into(),
    unit_price_minor: 5_000,
    campaign_id: None,
    comment: String::new(),
})?;
assert!(!cart.is_empty());
# Ok::<(), grabcli_core::Error>(())
```

This example makes no network calls. A cart price is only a display snapshot;
obtain a live quote through `Mart` before considering an order.

## License

[Apache-2.0](https://github.com/talaagent/grabcli/blob/master/LICENSE.txt)
