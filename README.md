# Reinforce

Reinforce is a self-hostable, domain-agnostic platform for source-grounded notes,
active-recall quizzing, timed visual exams and print-grade study sheet export.
It is built for an **AI-authored, human-reviewed** workflow: agents draft content
through the CLI or MCP server, and every draft passes a deterministic validator
and human review before it is published.

> Status: early development toward `0.1.0`.

## Repository layout

| Path | Purpose |
| --- | --- |
| `crates/reinforce-core` | Pure domain layer: SQLite storage, models, validator, FSRS, quiz engine |
| `crates/reinforce-export` | Embedded Typst PDF engine and lossless Markdown export |
| `crates/reinforce-server` | HTTP/WS API, embedded WebApp, MCP server and the `reinforce` CLI |
| `web/` | Touch-first WebApp (React, TypeScript, Vite, Tailwind CSS, i18n) |

## Development

Requirements: a recent stable Rust toolchain, Node.js 24+ and pnpm 9+.

```sh
# Rust workspace
cargo check --workspace
cargo test --workspace
cargo run -p reinforce-server -- serve --demo   # http://127.0.0.1:3000 with sample data

# WebApp
cd web
pnpm install
pnpm dev
pnpm build
```

The workspace version in the root `Cargo.toml` is the single source of truth
for every crate, the WebApp package and all documentation.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
