# grabfood

[![crates.io](https://img.shields.io/crates/v/grabfood.svg)](https://crates.io/crates/grabfood)
[![docs.rs](https://docs.rs/grabfood/badge.svg)](https://docs.rs/grabfood)

GrabFood typed API for [grabcli](https://github.com/talaagent/grabcli/blob/master/README.md): browse, menu, cart, quote,
vouchers, order, tracking and history, each returning plain structs.

This is an internal building block of the `grab` CLI, not a general-purpose
library. Its API is not stable.

## Modules

- `api`: endpoint URLs and the URL allowlist; the order-submission URL is only
  registered when ordering is enabled.
- `discover`, `browse`, `index`: search, nearby, recommended and category
  listings, their filters and paging, and catalog rows for the local cache
  ([docs/discovery.md](https://github.com/talaagent/grabcli/blob/master/docs/discovery.md)).
- `menu`, `cart`, `quote`: menu parsing, the cart (a value the caller keeps),
  and price quotes.
- `order`: gated order submission ([docs/checkout.md](https://github.com/talaagent/grabcli/blob/master/docs/checkout.md)).
- `tracking`: live order state as versioned JSON.
- `history`, `datetime`: export of completed orders to Markdown notes.

## How it fits

Built on [`grabcli-core`](https://docs.rs/grabcli-core) for session, auth and HTTP. The
[`grabcli`](https://docs.rs/grabcli) binary implements `grab food` on top of it. The `cli`
feature adds the bpaf parser for the list filters.

See the root README's [disclaimer](https://github.com/talaagent/grabcli/blob/master/README.md#disclaimer).

## Example

### read a merchant menu

```no_run
# async fn read_menu(ctx: &mut grabcli_core::Context) -> grabcli_core::Result<()> {
use grabfood::{menu, MenuSource};

let menu = menu(ctx, "merchant-id", MenuSource::Live).await?;
println!("{}", menu.name);
# Ok(()) }
```

Create `ctx` with `context` using your configuration, session store, and cache.
Reading a menu makes a network request but places no order. Use real merchant
IDs returned by browse calls. `cart_add` validates cart IDs against the menu;
`quote()` fetches current prices before checkout.

## License

[Apache-2.0](https://github.com/talaagent/grabcli/blob/master/LICENSE.txt)
