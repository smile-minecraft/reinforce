# reinforce-plugin-mineru

An official Reinforce plugin worker. It reads one job from stdin as a
`reinforce_plugin` v1 NDJSON `hello`, parses a PDF with an external MinerU CLI
and answers with a proposal batch of `resource`, `anchor` and draft
`knowledge_card` operations. The server validates the batch and stores it as a
pending proposal; nothing is published without owner approval.

## Requirements

- A MinerU executable available at an absolute path (`mineru parse` with
  `--json` and `--output`). The local 4.x CLI needs `mineru server start` (or a
  configured engine) before `parse` works.
- A writable absolute `--work-dir` for per-job markdown files. Each job gets a
  unique subdirectory that is removed when the job ends.
- No external tool is bundled or installed by this plugin.

The default `--tier flash` extracts text without a local layout engine. If a
full parse engine is installed, pass `--tier basic`, `standard` or `advanced`
in the plugin manifest arguments.

## Command line

| Flag | Required | Meaning |
| --- | --- | --- |
| `--mineru <path>` | yes | Absolute path to the MinerU executable. |
| `--work-dir <path>` | no | Absolute scratch directory; defaults to `TMPDIR`. |
| `--tier <name>` | no | `flash` (default), `basic`, `standard` or `advanced`. |
| `--wait <seconds>` | no | MinerU wait limit, default `180`. |

Startup exits with status 2 and a stderr message when a path is relative.
Tool failures during a job are reported as protocol `error` messages, never as
stderr-only failures.

## Job input

```json
{"pdf_path": "/absolute/path/to/paper.pdf", "title": "Paper", "subject": "physics"}
```

Only `pdf_path` is required; it must be an absolute path to an existing file.
`title` and `subject` are optional labels for the proposed resource and cards.

## Manifest example

```json
{
  "plugins": [
    {
      "name": "mineru",
      "capabilities": ["ingest_pdf"],
      "transport": {
        "type": "stdio",
        "command": "/absolute/path/to/target/debug/reinforce-plugin-mineru",
        "args": [
          "--mineru", "/Users/smile/.local/bin/mineru",
          "--work-dir", "/absolute/path/to/plugin-work"
        ]
      },
      "timeout_secs": 600,
      "max_output_bytes": 8388608,
      "max_attempts": 1
    }
  ]
}
```

## Output

For each parsed page the worker proposes one PDF-page anchor and one draft
knowledge card with a markdown block. One resource operation for the PDF comes
first, so the proposal targets that resource. Ids are generated as UUIDv7 by
the worker; no batch aliases are used. Error codes: `invalid_input`,
`external_tool_missing`, `external_tool_spawn_failed`, `external_tool_failed`,
`unparseable_output`, `unsupported_capability`, `protocol_mismatch`,
`empty_result` and `io_error`.

## Verification

`tests/e2e.rs` starts a real server with a fake MinerU script and covers the
successful pending proposal and the missing-tool error. A real run used this
command against a 682-byte, one-page PDF:

```sh
printf '%s\n' '{"type":"hello","protocol":"reinforce_plugin","version":1,"job_id":"real-mineru-probe","plugin":"mineru","capability":"ingest_pdf","input":{"pdf_path":"/absolute/path/probe.pdf"}}' \
  | timeout 600 ./target/debug/reinforce-plugin-mineru \
      --mineru /Users/smile/.local/bin/mineru --work-dir "$PWD/.scratch/work"
```

It answered `ready`, `progress` and `result` with three operations: a PDF
resource (sha256 `f15ad24c5d578431727ae22ed85578df14629399a5022eb0cbf456a97a251b31`,
682 bytes), a page-1 anchor and an unchecked draft card titled
`Reinforce MinerU probe` containing the extracted text.
