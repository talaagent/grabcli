# Contributing

Issues and pull requests are welcome.

## Development

With Nix, `nix develop` (or direnv) gives you the toolchain and installs the
pre-commit hooks. Before opening a PR, make sure these pass:

```
cargo fmt --all --check
cargo nextest run --workspace --all-features --locked
cargo test --workspace --all-features --doc --locked
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

Releases use **release-plz** to update release PRs and publish merged releases.
Release PRs use the built-in `GITHUB_TOKEN` by default, with job-level
`contents: write` and `pull-requests: write` permissions. Enable **Allow GitHub
Actions to create and approve pull requests** in the organization and repository
Actions settings, then set repository variable `RELEASE_PR=true`. The setup
check prevents a failing updater from disturbing an existing release PR.

GitHub requires a maintainer to approve CI runs on PRs created or updated with
`GITHUB_TOKEN`; token-generated pushes do not start ordinary push workflows.
An optional `RELEASE_PLZ_TOKEN` (GitHub App token or PAT) can override the built-in
token to run PR CI without that approval and support workflow-file updates.
No personal GitHub token is required for the default setup.

The `CARGO_REGISTRY_TOKEN` Actions secret is configured for registry publishing,
including new crates.

Pushes to `master` update a release PR with versions and changelogs. Review and
merge it after CI passes. `release_always = false` prevents publication from
ordinary pushes. Release-plz publishes in dependency order and creates tags
and GitHub releases. Approve the release PR CI runs before merging when using `GITHUB_TOKEN`.

For tokenless registry publication, configure each crate's crates.io trusted
publisher with owner `talaagent`, repository `grabcli`, workflow
`release-plz.yml`, environment `release`, then remove `CARGO_REGISTRY_TOKEN`.
Release-plz performs the OIDC exchange itself. New crates need a registry token
for first publication. Check all package versions and docs.rs builds afterward.

## License

grabcli is licensed under the [Apache License, Version 2.0](LICENSE.txt).
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be licensed under the Apache License,
Version 2.0, as described in section 5 of the license, without any additional
terms or conditions.

Use `jiff` for date/time handling; do not add a direct `chrono` dependency.
The Nix dev shell supplies Rust, Cargo, Clippy, rustfmt, and cargo-nextest.
