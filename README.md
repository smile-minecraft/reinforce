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

### Device sync

Native clients sync learning records with `reinforce serve` after answering
offline. Only attempts travel: `CardScheduling` is replayed from the attempt log
on every side, so the order of uploads never changes the result.

| Endpoint | Purpose |
| --- | --- |
| `POST /api/sync/attempts` | Upload a batch (`{"attempts": [...]}`). Idempotent by attempt id; the response reports `accepted`, `duplicates` and the account's current `cursor`. |
| `GET /api/sync/attempts?cursor=N&limit=M` | Pull the account's attempts after server cursor `N`, ordered by that cursor. |
| `PUT /api/sync/user-card-meta` | Upload stars, personal tags and notes; last writer wins on `updated_at`. |
| `GET /api/sync/user-card-meta` | Read every personal row of the account. |
| `GET /api/sync/content?known=<sha256>` | Published content as a `bundle.jsonl` document plus the blobs to download; `known` short-circuits an unchanged snapshot. |

Sync uses the same session cookie as the WebApp (the implicit owner in single-user
mode). Agent tokens are rejected on these endpoints, and every call is scoped to
the authenticated account: no endpoint accepts someone else's `user_id`.

```swift
let remoteUserId = "01a21714-c98d-7931-a48a-6a1d5cd348bd"
try database.bindRemoteUser(localUserId: localUserId, remoteUserId: remoteUserId)

let pending = try database.pendingSync(localUserId: localUserId, limit: 200)
// POST pending.attempts to /api/sync/attempts, then acknowledge only on 2xx:
try database.confirmUpload(localUserId: localUserId, attemptIds: ids, metaStamps: stamps)

// Pull every page and merge it; applying the same page twice is a no-op.
let merged = try database.applyRemoteBatch(
  localUserId: localUserId,
  attempts: page.attempts,
  userCardMeta: page.userCardMeta,
  nextCursor: page.nextCursor
)

// Content: write bundle_jsonl and the downloaded assets, then import.
let summary = try database.importContentSnapshot(bundleJsonl: bundle, assetsDir: assets)
```

A device profile created offline keeps its local `user_id`; `bindRemoteUser`
only records which server account it belongs to. Uploads are stamped with the
server id and pulled attempts are stored under the local profile, so both
devices end up with the same attempt set and the same FSRS numbers. Retries are
safe: `confirmUpload` is what marks a batch as sent, and `applyRemoteBatch`
deduplicates by attempt id and advances the pull cursor in one transaction.

Content snapshots follow the same `export_policy` rules as exports: `allow`
resources travel with their bytes, `redact` blocks are replaced by a notice, and
`owner_only` media plus the blocks that only exist for it never leave the
server. Blobs are downloaded through `/api/resources/{id}/blob`, which enforces
the same policy.

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
