# `reinforce-plugin-acp`

Official Reinforce external plugin that delegates study card drafting to any
[Agent Client Protocol (ACP)](https://agentclientprotocol.com) compatible AI
agent over stdio (JSON-RPC 2.0), strictly validates the returned card JSON, and
emits `reinforce_plugin` protocol v1 proposal operations.

Every generated card enters Reinforce as a **pending, unchecked Proposal** — it
never writes directly to the content store without owner approval.

## Installation requirements

1. **Reinforce plugin binary**: build from this workspace (or install the
   compiled binary to a fixed absolute path such as
   `/usr/local/bin/reinforce-plugin-acp`):

   ```sh
   cargo build -p reinforce-plugin-acp
   ```

2. **External ACP agent**: this plugin does **not** bundle or auto-install any
   AI agent. Install an ACP-compatible agent separately (for example,
   `opencode` invoked as `opencode acp`, or another ACP v1 stdio agent) and
   ensure it is authenticated and executable in the server environment.

   Remember that `reinforce serve` clears the plugin child environment to the
   allowlist `PATH`, `HOME`, `TMPDIR`, `LANG`, and `LC_ALL` (see
   `plugins/README.md`). Your external ACP agent must find its credentials and
   configuration using those variables (for example under `$HOME`).

## Manifest example

Pass the manifest file to `reinforce serve --plugins ./plugins.json` (or
`reinforce plugin list/submit --plugins ./plugins.json`):

```json
{
  "plugins": [
    {
      "name": "acp",
      "capabilities": ["draft_cards"],
      "transport": {
        "type": "stdio",
        "command": "/usr/local/bin/reinforce-plugin-acp",
        "args": [
          "--agent",
          "/opt/homebrew/bin/opencode",
          "--request-timeout-secs",
          "240",
          "--",
          "acp"
        ]
      },
      "timeout_secs": 300,
      "max_output_bytes": 8388608,
      "max_attempts": 1
    }
  ]
}
```

If you want jobs to reference local resource text files via
`input.resource_path`, explicitly confine file reads with `--resource-dir`:

```json
"args": [
  "--agent", "/opt/homebrew/bin/opencode",
  "--resource-dir", "/var/lib/reinforce/resources",
  "--",
  "acp"
]
```

## Command-line flags

| Flag | Required | Default | Description |
| --- | --- | --- | --- |
| `--agent <PATH>` | yes | — | Absolute path to the external ACP agent executable. Spawned directly without a shell. |
| `--cwd <DIR>` | no | current working dir | Working directory sent in ACP `session/new` (`cwd`) and used for the agent subprocess. |
| `--resource-dir <DIR>` | no | disabled | Directory confining `input.resource_path` reads. When omitted, `input.resource_path` is rejected. |
| `--capability <NAME>` | no | requested capability | Repeatable list of capabilities advertised in `ready`. When omitted, echoes the capability requested in `hello`. |
| `--request-timeout-secs <SECS>` | no | `240` | Per-request timeout for ACP `initialize`, `session/new`, and `session/prompt`. |
| `-- <ARGS...>` | no | `[]` | Arguments forwarded verbatim to the `--agent` executable. |

## Job input schema (`hello.input`)

Unknown fields are rejected. Supply **either** inline `content` **or**
`resource_path` (not both):

```json
{
  "resource_id": "019a0000-0000-7000-8000-000000000001",
  "resource_title": "Physics Ch. 3",
  "content": "Newton's third law states that for every action there is an equal and opposite reaction.",
  "subject": "physics",
  "topics": ["mechanics"],
  "anchor_ids": [],
  "instructions": "Draft concise review cards.",
  "max_cards": 10
}
```

| Field | Rules |
| --- | --- |
| `content` | Inline UTF-8 source text (non-empty, max 256 KiB; mutually exclusive with `resource_path`). |
| `resource_path` | File path confined inside `--resource-dir` (max 256 KiB; rejected if `--resource-dir` is not set or path escapes the directory). |
| `resource_id` | Optional identifier recorded in generated card `metadata.source_resource_id`. |
| `resource_title` | Optional source title included in the prompt context. |
| `subject` | Optional default subject (max 128 bytes); required on each proposed card if omitted here. |
| `topics` | Optional default topic list (max 32 items, 128 bytes each). |
| `anchor_ids` | Optional list of existing Reinforce anchor UUIDv7s attached to generated cards (max 32 items). |
| `instructions` | Optional extra prompt guidance (max 4 KiB). |
| `max_cards` | Optional upper bound on proposed cards (`1..=50`, default `50`). |

## Submitting a job via CLI

```sh
reinforce plugin submit \
  --db ./reinforce.db \
  --plugins ./plugins.json \
  --plugin acp \
  --capability draft_cards \
  --input '{"subject":"physics","topics":["mechanics"],"content":"Newton'\''s third law: forces occur in equal and opposite pairs."}'
```

## ACP protocol & security boundary

`reinforce-plugin-acp` implements the minimal ACP v1 client surface over
line-delimited JSON-RPC 2.0 on stdio:

1. `initialize` (`protocolVersion: 1`, `clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false }`)
2. `session/new` (`cwd`, `mcpServers: []`)
3. `session/prompt` (collects `session/update` `agent_message_chunk` text updates and emits throttled `progress` frames to Reinforce)
4. `session/cancel` when Reinforce sends `{"type":"cancel"}`, followed by a short grace wait and forced termination of the agent subprocess so cancellation never hangs.

Security boundaries and validation guarantees:

- **Protocol-level capability denial only (no OS sandbox)**: disabling ACP `fs`
  and `terminal` client capabilities, answering any `fs/*`, `terminal/*`, or
  other client-side method request from the agent with JSON-RPC error `-32601`,
  and rejecting `session/request_permission` (selecting `reject_always` /
  `reject_once` when offered, or `outcome: "cancelled"`) only restrict what the
  agent can request over the ACP protocol. This plugin does **not** place the
  agent in an operating-system sandbox: the `--agent` subprocess runs with the
  same OS permissions as the Reinforce server process and can still read or
  write files or execute commands directly using its own built-in tools. Only
  point `--agent` at a program you trust.
- **Strict proposal validation**: the agent's JSON output is bounded in size
  (512 KiB), parsed with `deny_unknown_fields`, checked for required fields,
  types, length limits, and choice/answer consistency (`knowledge_card` and
  `question_card`), and rejected with a structured plugin error before reaching
  the host if invalid.

## Structured error codes

When a run fails deterministically, `reinforce-plugin-acp` emits a protocol
`error` frame (`job.error.code = "plugin_reported"`,
`job.error.details.plugin_code = <code>`):

- `invalid_agent_path`: `--agent` is not an absolute path.
- `agent_not_found` / `agent_spawn_failed`: the `--agent` binary does not exist
  or failed to start.
- `invalid_input` / `resource_access_denied`: malformed `hello.input` or
  disallowed `resource_path`.
- `acp_protocol_error` / `acp_error` / `acp_timeout` / `acp_crashed` /
  `acp_output_too_large` / `acp_incomplete`: ACP handshake, session, prompt, or
  transport failure.
- `invalid_proposal`: the agent's response was not valid JSON or violated the
  card proposal schema/limits.
