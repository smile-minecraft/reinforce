# External plugins

Reinforce can hand heavy work — PDF layout analysis, AI agent sessions, TikZ
rendering — to a separate worker process. The server owns the queue, the
worker only produces a proposal batch. This document is the contract for that
boundary: who may declare a plugin, what gets executed, the JSON protocol and
how results reach the content store.

Version: `reinforce_plugin` protocol **1**, implemented by
`crates/reinforce-plugin-protocol` and `crates/reinforce-server/src/plugins`.

## Trust boundary

A plugin is a program running with the server's operating-system permissions.
The design keeps that power on the owner's side of the line:

- **Only the owner's configuration file declares a plugin.** The file is passed
  to `reinforce serve --plugins <path>`. No HTTP endpoint, database row, query
  parameter or job submission can create a plugin or change what a plugin runs.
- **The manifest is authoritative for the command.** `command` must be an
  absolute path, arguments are fixed strings from the file, and the process is
  spawned directly — never through a shell.
- **The child environment is cleared** and only `PATH`, `HOME`, `TMPDIR`,
  `LANG` and `LC_ALL` are passed through, so a worker cannot read unrelated
  server secrets from the environment.
- **Requests only choose**: an already configured plugin name, one of its
  declared capabilities and a JSON input object of at most 64 KiB. The
  submission body rejects unknown fields, so `{"command": ...}` is a `400`.
- **There is no sandbox.** A plugin can read and write anything the server
  process can. Treat installing a plugin like installing the server binary
  itself and only point `command` at programs you trust.
- **Remote (WS/HTTP) workers are not implemented.** The manifest parser
  recognises `{"type": "remote", "url": ...}` but loading the file fails with
  an explicit error, so a half-configured network worker can never run.

Ownership rules on the HTTP surface: every endpoint below requires an owner
session. A member session gets `403`; an agent token is not a session and gets
`401`. In `--auth single` mode the implicit local user is the owner.

## Manifest file

```json
{
  "plugins": [
    {
      "name": "mineru",
      "capabilities": ["ingest_pdf"],
      "transport": {
        "type": "stdio",
        "command": "/usr/local/bin/reinforce-plugin-mineru",
        "args": ["--stdio"]
      },
      "timeout_secs": 600,
      "max_output_bytes": 8388608,
      "max_attempts": 1
    }
  ]
}
```

| Field | Rules | Default |
| --- | --- | --- |
| `name` | lowercase letters, digits, `_` or `-`, at most 64 characters, unique | required |
| `capabilities` | 1–16 identifiers, unique; a submission must name one of them | required |
| `transport.type` | `stdio` only in this release; `remote` is rejected at load | required |
| `transport.command` | absolute path to the executable | required for `stdio` |
| `transport.args` | at most 64 arguments, each at most 4096 bytes, no NUL byte | `[]` |
| `timeout_secs` | 1–3600; wall-clock limit for one run | `300` |
| `max_output_bytes` | 1024–67108864; cap on everything the worker writes to stdout | `8388608` |
| `max_attempts` | 1–5; how often a transient failure may be retried | `1` |

Unknown fields are errors, so a typo cannot silently change what runs. The
server refuses to start if the manifest is missing, malformed or declares a
remote transport.

## Work queue

Each submission becomes a row in `plugin_jobs` (schema version 3). States:

```text
queued ──claim──▶ running ──result──▶ succeeded (proposal_id set)
   │                 │
   │                 ├──timeout / crash / spawn failure──▶ failed or queued (retry)
   │                 ├──protocol violation────────────────▶ failed
   │                 ├──validation failure────────────────▶ failed (details = report)
   │                 ├──plugin `error` message────────────▶ failed (details.plugin_code)
   │                 └──cancel────────────────────────────▶ cancelled
   └──cancel────────────────────────────────────────────────▶ cancelled
```

- **One worker at a time.** A single dispatcher runs the oldest queued job and
  polls every 250 ms when the queue is empty. A job queued by the CLI is picked
  up by a running server without any extra step.
- **Timeouts end the process.** When `timeout_secs` elapses the child is killed
  and reaped (`kill_on_drop` is a backstop), and the job fails with `timeout`.
- **Output is counted.** Everything read from stdout counts against
  `max_output_bytes`; exceeding it kills the worker and fails with
  `output_too_large`. Stderr is not part of the protocol and only the last 8 KiB
  are kept for the error details.
- **Cancellation is real.** Cancelling a queued job marks it `cancelled`
  immediately. Cancelling a running job sends `{"type":"cancel"}`, waits a short
  grace period, then kills and reaps the child before recording `cancelled`.
- **Retries are bounded.** Only transient failures retry (`timeout`, `crashed`,
  `spawn_failed`, `internal`) and only while `attempts < max_attempts`.
  Protocol violations, plugin errors and validation failures are deterministic
  and fail immediately.
- **Restart is predictable.** A server started with a manifest settles the
  queue first: every job still marked `running` moves to `failed` with error
  code `interrupted` (its child died with the old process and cannot be
  resumed), `queued` jobs stay queued and run again, and jobs of plugins that
  are no longer declared fail with `plugin_not_configured`. A process started
  without a manifest (`reinforce export`, `reinforce mcp`) leaves the queue
  untouched, so it can never fail another process's running job.

Job errors use a closed code set: `timeout`, `crashed`, `output_too_large`,
`protocol_violation`, `spawn_failed`, `validation_failed`, `interrupted`,
`cancelled`, `plugin_not_configured`, `plugin_reported`, `internal`. A plugin's
own error code is kept in `error.details.plugin_code`.

## Protocol v1 (stdio, NDJSON)

The host writes one JSON object per line to the worker's stdin and reads one
JSON object per line from its stdout. Lines are UTF-8; stderr is free-form.

### 1. Host → worker: `hello`

```json
{"type":"hello","protocol":"reinforce_plugin","version":1,
 "job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd",
 "plugin":"mineru","capability":"ingest_pdf",
 "input":{"resource_id":"01a21714-c98c-7bfe-8cf8-a60b84573f56"}}
```

### 2. Worker → host: `ready`

```json
{"type":"ready","protocol":"reinforce_plugin","version":1,
 "job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd",
 "plugin":"mineru","capabilities":["ingest_pdf"]}
```

`protocol` and `version` must match exactly and `job_id` must echo the hello;
`plugin` must be the name the owner configured. Any other value fails the job
with `protocol_violation` before any content is staged. `capabilities` may be
empty; when present it must contain the requested capability.

### 3. Worker → host: `progress` (zero or more)

```json
{"type":"progress","job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd",
 "fraction":0.5,"message":"page 120 of 240"}
```

`fraction` is clamped into `0..=1`. Progress is stored on the job row and is
visible through the status CLI and API.

### 4. Worker → host: exactly one terminal message

```json
{"type":"result","job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd",
 "operations":[
   {"op":"knowledge_card","temp_ref":"$card_1",
    "data":{"id":"$card_1","subject":"physics","topics":["mechanics"],
            "title":"Newton's third law","blocks":[],"anchor_ids":[],
            "status":"draft","verification":"unchecked","unsure_count":0,
            "metadata":{},
            "created_at":"2026-11-27T00:00:00Z","updated_at":"2026-11-27T00:00:00Z"}}
 ]}
```

```json
{"type":"error","job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd",
 "code":"unsupported_page","message":"page 12 has no text layer"}
```

`operations` uses the same shape as the standard batch format
(`docs/SPEC.md` §5.3): `op` is one of `resource`, `anchor`, `knowledge_card`,
`note`, `exam_set`, `question_group`, `question_card`, `edge`; `temp_ref` is an
optional `$alias` resolved inside the batch; `data` is the entity payload.
Operations must be non-empty.

### 5. Host → worker: `cancel`

```json
{"type":"cancel","job_id":"01a21714-c98d-7931-a48a-6a1d5cd348bd"}
```

Best effort: a worker that does not exit within the grace period is killed.

### Ordering rules

`ready` first, then zero or more `progress`, then exactly one `result` or
`error`. EOF without a terminal message is `crashed`. A second `ready`, an
unknown `type`, non-JSON output, a wrong `job_id` or an unadvertised capability
is `protocol_violation`. A worker may exit as soon as it has written its
terminal message; the host waits briefly, then kills a lingering process.

## Results become proposals

The host converts `operations` into the core batch representation and calls the
same `proposal::submit_batch` path as the MCP tool, with
`Actor::Agent { name: "plugin:<plugin>" }`:

- the new proposal is **pending** and waits for owner review;
- the agent actor forces `verification = unchecked` on knowledge and question
  cards, even when a worker asks for `verified`;
- nothing is written to the content store by the plugin path itself — only an
  owner approval applies a proposal;
- a batch that fails domain validation fails the job with
  `validation_failed` and stores the full field-level report in
  `error.details.errors`. Submission is atomic, so a rejected batch writes
  nothing at all.

## CLI

```sh
# read the manifest and print the configured plugins
reinforce plugin list --plugins ./plugins.json

# queue one job (durable: a running server picks it up)
reinforce plugin submit --db ./reinforce.db --plugins ./plugins.json \
  --plugin mineru --capability ingest_pdf --input '{"resource_id":"..."}'
# --input also accepts @file.json or - for stdin

# inspect one job, or list recent ones
reinforce plugin status --db ./reinforce.db --job <job-id> [--wait 120]
reinforce plugin status --db ./reinforce.db --status failed --limit 20
```

`--db` and `--plugins` are accepted before or after the subcommand.

## HTTP API

All endpoints are owner-only.

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/api/plugins` | configured plugins, their capabilities, command, limits |
| `POST` | `/api/plugins/{name}/jobs` | queue `{"capability": "...", "input": {...}}` → `202` with the job |
| `GET` | `/api/plugins/jobs?status=&plugin=&limit=` | list jobs, newest first |
| `GET` | `/api/plugins/jobs/{id}` | one job with status, attempts, progress, proposal id and error |
| `POST` | `/api/plugins/jobs/{id}/cancel` | cancel a queued or running job |

There is no route that creates, edits or deletes a plugin.

## Tests

`crates/reinforce-server/tests/plugins.rs` builds
`tests/fixtures/fake_plugin.rs` with `rustc` at test time and runs real child
processes. The suite covers the happy path (queue → run → pending, unchecked
proposal), timeout with a killed child, crash, oversized output, protocol
violations, cancel of a running and a queued job, restart recovery, orphaned
jobs, retry limits, plugin-reported errors, validation failures, owner/member/
agent-token permissions, the absence of manifest-mutating routes, the CLI, and
the refusal of remote transports. No test needs the network.
