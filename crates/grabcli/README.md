# grabcli

[![crates.io](https://img.shields.io/crates/v/grabcli.svg)](https://crates.io/crates/grabcli)
[![docs.rs](https://docs.rs/grabcli/badge.svg)](https://docs.rs/grabcli)

The `grab` binary: an unofficial command-line client for Grab, starting with
GrabFood and GrabMart.

## Install

```
nix build .#grab
# or
cargo install grabcli --locked
# Or build the current source:
cargo install --git https://github.com/talaagent/grabcli grabcli --locked
```

Commands, auth setup and the ordering safety model are documented in the
[root README](https://github.com/talaagent/grabcli/blob/master/README.md). Read its [disclaimer](https://github.com/talaagent/grabcli/blob/master/README.md#disclaimer)
first.

## How it fits

This crate parses arguments, prints, and owns the files: `config.toml`,
`session.json`, carts, login state and the SQLite cache
([`grabcli-sqlite`](https://docs.rs/grabcli-sqlite)). Its `grab food` and `grab mart`
commands call the typed [`grabfood`](https://docs.rs/grabfood) and
[`grabmart`](https://docs.rs/grabmart) libraries. Shared session, auth and HTTP code lives
in [`grabcli-core`](https://docs.rs/grabcli-core).

## License

[Apache-2.0](https://github.com/talaagent/grabcli/blob/master/LICENSE.txt)
