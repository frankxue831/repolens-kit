# Contributing to RepoLens

Thanks for your interest. RepoLens is a small, single-maintainer project, so please open an issue before starting anything larger than a bug fix or a docs tweak.

## Getting set up

You need a recent stable Rust toolchain (install it with [rustup](https://rustup.rs)).

```sh
cargo build
cargo test --workspace
cargo run -- scan --root .
```

## Before you open a pull request

CI runs the same gate, so run it locally first:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Snapshot tests use [insta](https://insta.rs). If you change output on purpose, review the snapshots with `cargo insta review` and commit the updated `.snap` files.

If you change anything under `docs/` or `fixtures/`, also run `bash scripts/verify-all.sh`, which runs the same doc-canon and fixture checks as the Doc Canon CI job. If you change `scripts/verify-doc-canon.sh` itself, also run `bash scripts/test-verify-doc-canon.sh`. These scripts need bash 4 or newer and Ruby. macOS ships bash 3.2, so install a newer bash first, for example with `brew install bash`.

## Pull request guidelines

- Keep each PR focused on one change, and link the issue it addresses.
- Add or update tests for any behavior change.
- Update `README.md` or `docs/` when you change a command, flag, or MCP tool.
- Be honest about status. If something is partial, scaffolding, or warnings-only, say so in docs the way the README does.
- Don't commit absolute local paths, credentials, or personal data.

## Using AI coding agents

Agent-assisted contributions are welcome. `AGENTS.md` and `CLAUDE.md` hold the in-repo rules agents should follow. You're responsible for reviewing and testing everything you submit.

## Reporting bugs and security issues

Use the issue templates for bugs and feature requests. For security problems, follow [SECURITY.md](SECURITY.md) and don't open a public issue.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
