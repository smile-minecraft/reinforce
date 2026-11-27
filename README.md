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
| `crates/reinforce-ffi` | Synchronous UniFFI bindings for offline Swift and Kotlin clients |
| `crates/reinforce-export` | Embedded Typst PDF engine and lossless Markdown export |
| `crates/reinforce-server` | HTTP/WS API, embedded WebApp, MCP server and the `reinforce` CLI |
| `web/` | Touch-first WebApp (React, TypeScript, Vite, Tailwind CSS, i18n) |

## Development

Requirements: a recent stable Rust toolchain, Node.js 24+ and pnpm 9+.

```sh
# WebApp: build it first, the server embeds web/dist
cd web
pnpm install
pnpm build
cd ..

# Rust workspace
cargo check --workspace
cargo test --workspace
cargo run -p reinforce-server -- serve --demo   # http://127.0.0.1:3000 with sample data
```

`web/dist` is not committed. Without it the server shows a placeholder page;
run `pnpm build` in `web/` and reload. Use `pnpm dev` in `web/` for frontend
work with hot reload.

### Mobile bindings

`crates/reinforce-ffi` exposes a synchronous UniFFI API over `reinforce-core`.
Bindings are generated in library mode from the built `cdylib`; generated files
are written to `bindings/` and are not committed:

```sh
./scripts/generate-bindings.sh            # defaults to bindings/
./scripts/generate-bindings.sh /tmp/reinforce-bindings
```

The script builds `reinforce-ffi --release`, generates Swift and Kotlin sources
with `uniffi-bindgen`, and type-checks the Swift source with `swiftc` when
available. On macOS the library is `target/release/libreinforce_ffi.dylib`;
on Linux it is `target/release/libreinforce_ffi.so`.

The bindings cover the whole offline study flow, not just single reads:

```swift
let database = try LocalDatabase.openWithBlobs(databasePath: db, blobsDir: media)
let summary = try database.importBundle(bundlePath: demoBundle)
let subjects = try database.subjects()
let notes = try database.notes(subject: "physics", limit: 50)
let outline = try database.noteOutline(noteId: noteId)
let examSets = try database.examSets(subject: "physics", limit: 50)
let questions = try database.questionCards(examSetId: examSetId, limit: 100)
let neighbors = try database.neighbors(endpoint: .card(id: cardId), depth: 1)
let path = try database.resourceBlobPath(resourceId: resourceId)
```

`importBundle` accepts a `bundle.jsonl` file or a bundle directory and copies
the `assets/` files its resource operations reference into the blob directory
under their sha256. Blobs are content-addressed and never rewritten, so
re-importing the same bundle is safe and reports `blobsWritten == 0`.
`open(path)` keeps the blob directory in `blobs/` beside the database;
`openWithBlobs` points it at an app-group or sandbox location. `resource` and
`anchor` expose media metadata, `resourceBlobPath` returns the absolute local
file to render, and `userCardMeta`/`saveUserCardMeta` keep stars, personal tags
and notes. Library lists return published content only; `examSets` reports both
the total and published question counts of each set.

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
