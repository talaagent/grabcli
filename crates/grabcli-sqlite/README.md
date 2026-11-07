# grabcli-sqlite

[![crates.io](https://img.shields.io/crates/v/grabcli-sqlite.svg)](https://crates.io/crates/grabcli-sqlite)
[![docs.rs](https://docs.rs/grabcli-sqlite/badge.svg)](https://docs.rs/grabcli-sqlite)

The SQLite cache backend for [grabcli](https://github.com/talaagent/grabcli/blob/master/README.md): `SqliteCache`
implements [`grabcli-core`](https://docs.rs/grabcli-core)'s `Cache` trait with one database
(WAL, mode 0600) holding cached responses, a restaurant/menu catalog with FTS5
and trigram search, and a content-addressed image store with LRU eviction.

This is an internal building block of the `grab` CLI, not a general-purpose
library. Its API is not stable. It is the only crate that links `rusqlite`.

See the root README's [disclaimer](https://github.com/talaagent/grabcli/blob/master/README.md#disclaimer).

## Example

### configure a cache

```no_run
use grabcli_core::cache::CacheConfig;
use grabcli_sqlite::SqliteCache;

let cache = SqliteCache::new("/path/to/private/cache", CacheConfig::default());
let stats = cache.stats()?;
println!("cached responses: {}", stats.entries);
# Ok::<(), grabcli_core::Error>(())
```

The first operation opens or creates the database. Give each application its
own private cache directory. Pass an `Arc<SqliteCache>` as the core [Cache](https://docs.rs/grabcli-core/latest/grabcli_core/trait.Cache.html)
implementation when creating a context.

## License

[Apache-2.0](https://github.com/talaagent/grabcli/blob/master/LICENSE.txt)
