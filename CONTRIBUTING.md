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

The release workflow dispatches CI explicitly for PRs created with `GITHUB_TOKEN`,
using `actions: write`. It reuses existing runs for the same commit. This uses
GitHub's [workflow dispatch exception](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)
to the token's event restrictions. An optional `RELEASE_PLZ_TOKEN` (GitHub App
token or PAT) can override the built-in token; no personal token is required.

Registry publication uses crates.io OIDC trusted publishing; the release job
does not receive a persistent Cargo registry token.

Pushes to `master` update a release PR with versions and changelogs. Review and
merge it after CI passes. `release_always = false` prevents publication from
ordinary pushes. Release-plz publishes in dependency order and creates tags
and GitHub releases.



## License

grabcli is licensed under the [Apache License, Version 2.0](LICENSE.txt).
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you shall be licensed under the Apache License,
Version 2.0, as described in section 5 of the license, without any additional
terms or conditions.

Use `jiff` for date/time handling; do not add a direct `chrono` dependency.
The Nix dev shell supplies Rust, Cargo, Clippy, rustfmt, and cargo-nextest.


## Trusted publishing

Each crate trusts GitHub owner `talaagent`, repository `grabcli`, workflow
`release-plz.yml`, environment `release`. The publishing job has `id-token: write`;
release-plz performs the OIDC exchange itself. Do not set `CARGO_REGISTRY_TOKEN`
in that job: a persistent token overrides OIDC.

Normal pushes publish only merged release PR versions. For an explicitly
requested initial rollout or recovery, dispatch the release workflow with
`bootstrap=true`; it publishes the committed versions through release-plz.
`release_packages` optionally selects a comma-separated subset, allowing existing
dependencies to publish before a new workspace crate. Empty means the workspace.
The generated configuration is temporary and does not weaken normal push gating.

For a brand-new crate, use a short-lived token with `publish-new` for its first
release, then register the same trusted publisher through the crates.io settings
or its API using `trustpub` permission. Never commit the token. Future updates
use OIDC.
