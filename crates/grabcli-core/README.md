# grabcli-core

[![crates.io](https://img.shields.io/crates/v/grabcli-core.svg)](https://crates.io/crates/grabcli-core)
[![docs.rs](https://docs.rs/grabcli-core/badge.svg)](https://docs.rs/grabcli-core)

Shared session, auth and HTTP plumbing for [grabcli](https://github.com/talaagent/grabcli/blob/master/README.md).

This is an internal building block of the `grab` CLI, not a general-purpose
library. Its API is not stable.

## Modules

- `config`: `config.toml` settings (parsed from text), including the
  `allow_ordering` gate.
- `session`, `store`: Grab cookies and hydra token, and the async
  `SessionStore` trait the context loads and saves them through.
- `cdp`: Chrome DevTools Protocol client for importing a logged-in browser's
  session and minting hydra.
- `sso`, `login`: silent session refresh through the GrabID SSO cookie, and
  the browser-driven `grab auth login` ([docs/auth.md](https://github.com/talaagent/grabcli/blob/master/docs/auth.md)).
- `cache`: the async `Cache` backend trait (responses, catalog, images) and
  `CacheHandle` (TTLs, modes), for read-only browse data only. The SQLite
  backend is [`grabcli-sqlite`](https://docs.rs/grabcli-sqlite).
- `hydra`, `jwt`: the hydra device JWT ([docs/hydra.md](https://github.com/talaagent/grabcli/blob/master/docs/hydra.md)).
  Native RS256 minting needs the non-default `native-hydra` feature (rsa).
- `http`: the HTTP client; every request is checked against a URL allowlist.
- `ctx`: `Context::new(config, store, cache)`: owns config, session and
  client, saves session changes through the store, and re-mints hydra on
  rejection.
- `error`: one error type with stable kinds and exit codes.
- `location`, `money`: delivery address, centavo amounts.
- `places`: the account's saved places (one read-only GET) and the `location`
  cookie for one ([docs/places.md](https://github.com/talaagent/grabcli/blob/master/docs/places.md)).

## How it fits

Service crates such as [`grabfood`](https://docs.rs/grabfood) and
[`grabmart`](https://docs.rs/grabmart) register their allowlisted URLs
and make every call through `Context`. The library owns no storage: no paths,
config files, environment or printing. The [`grabcli`](https://docs.rs/grabcli) binary
wires files, the SQLite cache and the services together.

See the root README's [disclaimer](https://github.com/talaagent/grabcli/blob/master/README.md#disclaimer).

## Example

### format a price

```
use grabcli_core::Money;

let price = Money(12_345); // integer centavos, not floating-point pesos
assert_eq!(price.peso(), "₱123.45");
assert_eq!(price.minor(), 12_345);
```

For HTTP calls, construct a `Context` with your own `SessionStore` and
`Cache`; see `grabfood::context` or `grabcli-service` for service-level setup.

## License

[Apache-2.0](https://github.com/talaagent/grabcli/blob/master/LICENSE.txt)
