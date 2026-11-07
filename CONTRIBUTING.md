# Contributing

Issues and pull requests are welcome.

## Development

With Nix, `nix develop` (or direnv) gives you the toolchain and installs the
pre-commit hooks. Before opening a PR, make sure these pass:

```
cargo fmt --all --check
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo clippy --workspace --all-targets -- -D warnings
nix fmt
nix flake check
```

Arguments are parsed with `bpaf` (derive). Money is integer minor units.

## Ground rules

- Keep the ordering safety gates intact: the submit URL is only allowlisted
  with `allow_ordering = true`, `grab food order` / `grab mart order` need
  `--yes <quote-id>` or an interactive confirm, and the submit endpoint is
  never retried.
- Test fixtures must be synthetic. Never commit cookies, tokens, session files,
  captured network traffic, real addresses or coordinates, phone numbers, order
  or booking ids, or payment details. Scrub anything recorded from the live API
  before adding it.
- Be gentle with Grab's API: no tight polling, back off on `429`.

## Releasing

Releases use [release-plz](https://release-plz.dev). A maintainer must first
publish missing crates in dependency order with `cargo publish -p <crate>`;
trusted publishing cannot create a crate. `grabcli-sqlite` and
`grabcli-service` need this bootstrap. Supply registry credentials through
`cargo login`, never in a commit or issue.

For every crate, configure a crates.io trusted publisher with owner `talaagent`,
repository `grabcli`, workflow `release-plz.yml`, environment `release`.
The organization and repository must allow Actions to create pull requests;
then set repository variable `RELEASE_PR=true`. Until those prerequisites are
met, release automation is incomplete. Missing crates or authentication failures
fail the release job rather than silently skipping publication.

After setup, pushes to `master` update a release PR with versions and changelogs.
Review and merge it after CI passes to publish. `release_always = false` keeps
ordinary pushes from publishing. PRs made with `GITHUB_TOKEN` do not trigger CI
automatically; maintainers must ensure the release branch is checked before
merging. Check all workspace packages on crates.io and their docs.rs build status
after each release.

## License

grabcli is licensed under the [Apache License, Version 2.0](LICENSE.txt).
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be licensed under the Apache License,
Version 2.0, as described in section 5 of the license, without any additional
terms or conditions.
