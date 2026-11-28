# Reinforce

Reinforce is a self-hostable, domain-agnostic platform for source-grounded notes,
active-recall quizzing, timed visual exams and print-grade study sheet export.
It is built for an **AI-authored, human-reviewed** workflow: agents draft content
through the CLI or MCP server, and every draft passes a deterministic validator
and human review before it is published.

> Status: `0.2.0` — native clients and plugins. On top of the `0.1.0` server,
> WebApp and CLI, this release adds offline UniFFI bindings, a 2D canvas view
> for notes, device study sync, a native SwiftUI app for iPhone and iPad, a
> GPUI desktop reader and quiz runner, an out-of-process plugin queue and the
> official MinerU, TikZ and ACP plugins. Android is planned for a later release.

## Repository layout

| Path | Purpose |
| --- | --- |
| `crates/reinforce-core` | Pure domain layer: SQLite storage, models, validator, FSRS, quiz engine |
| `crates/reinforce-desktop` | Native GPUI desktop reader and quiz runner for a local database |
| `crates/reinforce-ffi` | Synchronous UniFFI bindings for offline Swift and Kotlin clients |
| `crates/reinforce-export` | Embedded Typst PDF engine and lossless Markdown export |
| `crates/reinforce-plugin-protocol` | Versioned JSON messages shared with out-of-process plugins |
| `crates/reinforce-server` | HTTP/WS API, embedded WebApp, MCP server and the `reinforce` CLI |
| `plugins/` | Protocol documentation and the official MinerU, TikZ and ACP workers |
| `web/` | Touch-first WebApp (React, TypeScript, Vite, Tailwind CSS, i18n) |
| `apps/ios/` | Native offline SwiftUI app for iPhone and iPad using `reinforce-ffi` |

### Official plugins

`plugins/reinforce-plugin-mineru` turns a PDF into a resource, page anchors and
draft cards through an external MinerU CLI.
`plugins/reinforce-plugin-tikz` renders TikZ source into an SVG resource and a
diagram card through LaTeX and dvisvgm.
`plugins/reinforce-plugin-acp` hands a resource to an external AI agent over the
Agent Client Protocol and turns the cards it proposes into a proposal batch.
All three are standalone stdio binaries that speak the protocol in
`plugins/README.md`; see each crate's README for tool requirements and manifest
arguments. Plugin output always arrives as a pending proposal for human review.

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
| `GET /api/sync/attempts?cursor=N&limit=M` | Pull the account's attempts after server cursor `N`, ordered by that cursor. `limit` is a soft bound: a page always carries every row that shares the last cursor, so a caller that advances to `next_cursor` never skips one. |
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

Send `submitted_payload` back exactly as the device produced it. The server
compares the stored payload with the uploaded one and answers `409` when they
differ, so re-encoding it on the way out (for example turning `0` into `0.0`)
would turn a harmless retry into a conflict.

Pulled attempts and personal rows are recorded as already uploaded, so a device
does not echo back what it just downloaded. Content snapshots never carry the
title of a `redact` resource, and a note's canvas layout only keeps the cards
the viewer actually received.

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

### Desktop app (GPUI)

`crates/reinforce-desktop` is a native macOS window that links `reinforce-core`
directly — no server, no FFI — and reads a local `reinforce.db` plus its `blobs`
directory. It renders notes and knowledge cards in a reading room, pre-renders
math and diagrams to SVG with the embedded Typst engine, and runs all five study
modes (daily due, full exam, topic drill, practical exam, weakness drill). Every
finished session is written to the append-only attempt log and refreshes FSRS
scheduling through the same core functions the CLI uses.

It is excluded from `scripts/verify.sh` and from the Linux CI job: GPUI needs
macOS, a GUI toolkit and a full Metal toolchain, and compiling it adds several
minutes. Check it on a Mac instead:

```sh
./scripts/check-desktop.sh
```

Building GPUI requires the Metal toolchain once per Xcode installation:

```sh
xcodebuild -downloadComponent MetalToolchain   # one-time, ~840 MB
```

Run it against the demo library (the import happens before the window opens):

```sh
CARGO_BUILD_JOBS=4 cargo run -p reinforce-desktop -- \
  --db /tmp/reinforce-desktop/reinforce.db \
  --import fixtures/demo \
  --practical-exam 2026-12-20
```

Useful flags: `--db`, `--blobs`, `--cache`, `--import`, `--practical-exam
YYYY-MM-DD`, `--start reading|card:<n>|exam|daily|topic|practical|weakness` and
`--select A,C` (preselect option keys, handy for screenshots). `--help` prints
the full list.

Keyboard: `j`/`k` move through notes, `1`–`5` start a study mode, `A`–`Z` toggle
answer options, `Enter` submits (and hands in a finished paper), `Backspace`
edits a typed answer, `Esc` returns. The header shows the practical exam
countdown; the footer shows the database path, the attempt count and the keys.

Pin-locate questions require free positioning on the image; the desktop build
records a blank submission for them and leaves the coordinate input to the
touch-first WebApp.

The workspace version in the root `Cargo.toml` is the single source of truth
for every crate, the WebApp package and all documentation.

### iOS and iPadOS app (`apps/ios`)

The native SwiftUI client in `apps/ios/` supports iPhone and iPad from a single
target and runs completely offline against `reinforce-core` via UniFFI and local
SQLite. It also supports optional server sync (`Server & Sync`) using session
cookies stored in iOS Keychain, automatic foreground / reconnect sync with
offline queuing, server-rendered PDF export (`standard`, `goodnotes`,
`worksheet`) via `/api/export` and the iOS share sheet, and `reinforce://card/<id>`
/ `reinforce://question/<id>` / `reinforce://note/<id>` deep links. The Xcode
project is generated with [XcodeGen](https://github.com/yonaskolb/XcodeGen)
from `apps/ios/project.yml` and is not committed to git.

Requirements: Xcode (with an iOS Simulator runtime installed), `xcodegen`, and
`rustup` with the `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets.

```sh
# 1. Build debug iOS static libraries, Swift bindings, and ReinforceFFI.xcframework
CARGO_BUILD_JOBS=4 ./scripts/build-ios-ffi.sh

# 2. Generate apps/ios/Reinforce.xcodeproj
(cd apps/ios && xcodegen generate)

# 3. Build and run unit + UI tests on an iPhone or iPad simulator
xcodebuild test \
  -project apps/ios/Reinforce.xcodeproj \
  -scheme Reinforce \
  -derivedDataPath apps/ios/build/DerivedData \
  -destination 'platform=iOS Simulator,name=iPhone 18 Pro'
```

Open `apps/ios/Reinforce.xcodeproj` in Xcode to run interactively on an iPhone
or iPad simulator (or choose any available iPhone/iPad destination listed by
`xcrun simctl list devices available`). Pass `--release` to
`./scripts/build-ios-ffi.sh` when building optimized release libraries.

### External plugins

Heavy work (PDF layout analysis, agent sessions, TikZ rendering) can run in a
separate worker process. The owner declares the workers in a manifest file —

```json
{
  "plugins": [
    {
      "name": "mineru",
      "capabilities": ["ingest_pdf"],
      "transport": {"type": "stdio", "command": "/usr/local/bin/reinforce-plugin-mineru"}
    }
  ]
}
```

— and starts the server with `reinforce serve --plugins ./plugins.json`. Only
that file decides which programs may run: the HTTP API and the CLI can queue
work for a declared plugin, read its status and cancel it, and nothing more.

```sh
reinforce plugin list --plugins ./plugins.json
reinforce plugin submit --db ./reinforce.db --plugins ./plugins.json \
  --plugin mineru --capability ingest_pdf --input '{"resource_id":"..."}'
reinforce plugin status --db ./reinforce.db --job <job-id> --wait 120
```

Workers speak a versioned newline-delimited JSON protocol over stdio and return
a proposal batch. The batch is submitted as an agent actor, so everything it
produces stays `unchecked` and pending until the owner reviews it; a worker can
never publish content directly. Jobs are durable across restarts, with a
timeout, an output cap and cancellation per job. The trust boundary, the wire
format and the queue state machine are documented in [`plugins/README.md`](plugins/README.md).

### MCP server

`reinforce` ships with a built-in Model Context Protocol (MCP) server over both
stdio (for local agents) and Streamable HTTP at `/mcp` (when running
`reinforce serve`).

Available tools:
- **Browse & read**: `subject_group_list`, `note_list`, `note_get`, `exam_set_list`,
  `resource_list`, `resource_read_page`, `card_search`, `card_get`, `question_get`,
  `card_get_graph`
- **Diagnosis**: `study_weakness_report`
- **Proposals**: `proposal_submit_batch`, `proposal_list`, `proposal_get`

Local stdio client configuration:

```json
{
  "mcpServers": {
    "reinforce": {
      "command": "/usr/local/bin/reinforce",
      "args": ["mcp", "--db", "/path/to/reinforce.db", "--blobs", "/path/to/blobs", "--agent-name", "claude"]
    }
  }
}
```

Remote Streamable HTTP configuration (in `--auth invite` mode, create a named
agent token as the owner via `POST /api/auth/agent-tokens` with `{"name": "claude"}`
and pass it as a Bearer token; proposals record `submitted_by` as `agent:<name>`):

```json
{
  "mcpServers": {
    "reinforce": {
      "url": "https://reinforce.example.org/mcp",
      "headers": {
        "Authorization": "Bearer <agent-token>"
      }
    }
  }
}
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
