# reinforce-plugin-tikz

An official Reinforce plugin worker. It reads one `reinforce_plugin` v1 NDJSON
`hello`, renders TikZ source with an external LaTeX + dvisvgm pair and answers
with an SVG `resource` plus a draft diagram `knowledge_card`. The server
validates the batch and stores it as a pending proposal; nothing is published
without owner approval.

## Requirements

- A LaTeX executable with TikZ and the `standalone` class available at an
  absolute path (for example `/usr/local/bin/latex` from TeX Live).
- An absolute path to `dvisvgm`.
- A writable absolute `--work-dir` for the temporary `.tex`, `.dvi` and `.svg`
  files. Each job gets a unique subdirectory that is removed when the job ends.
- No external tool is bundled or installed by this plugin.

LaTeX runs with `-no-shell-escape`; the worker assembles an argument vector and
never invokes a shell. On this development machine neither `latex` nor
`dvisvgm` is installed, so the successful path is verified only with fake
executables in `tests/e2e.rs`, not with a real render.

## Command line

| Flag | Required | Meaning |
| --- | --- | --- |
| `--latex <path>` | yes | Absolute path to the LaTeX executable. |
| `--dvisvgm <path>` | yes | Absolute path to the dvisvgm executable. |
| `--work-dir <path>` | no | Absolute scratch directory; defaults to `TMPDIR`. |

Startup exits with status 2 and a stderr message when a path is relative.
Tool failures during a job are reported as protocol `error` messages, never as
stderr-only failures.

## Job input

```json
{
  "source": "\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}",
  "preamble": "\\usetikzlibrary{arrows.meta}",
  "title": "Triangle",
  "subject": "geometry"
}
```

Only `source` is required. It is raw TikZ content, not a full LaTeX document:
the worker wraps it in a `standalone` document. `preamble` is optional LaTeX
inserted after `\usepackage{tikz}`. `title` and `subject` are optional labels.

## Manifest example

```json
{
  "plugins": [
    {
      "name": "tikz",
      "capabilities": ["tikz_render"],
      "transport": {
        "type": "stdio",
        "command": "/absolute/path/to/target/debug/reinforce-plugin-tikz",
        "args": [
          "--latex", "/usr/local/bin/latex",
          "--dvisvgm", "/usr/local/bin/dvisvgm",
          "--work-dir", "/absolute/path/to/plugin-work"
        ]
      },
      "timeout_secs": 300,
      "max_output_bytes": 8388608,
      "max_attempts": 1
    }
  ]
}
```

Keep `timeout_secs` high enough for LaTeX's first run to build font maps.

## Output

The first operation is an `image` resource for the SVG, with a sha256 of the
SVG bytes and dimensions read from the SVG attributes or `viewBox`. The second
is a draft knowledge card whose `diagram` block keeps the original TikZ source
and the rendered SVG in `svg_cache`. Ids are generated as UUIDv7 by the worker;
no batch aliases are used. Error codes: `invalid_input`,
`external_tool_missing`, `external_tool_spawn_failed`, `external_tool_failed`,
`unparseable_output`, `unsupported_capability`, `protocol_mismatch`,
`empty_result` and `io_error`.

## Verification

`tests/e2e.rs` starts a real server with fake `latex` and `dvisvgm` scripts and
covers the successful pending proposal and the missing-tool error. A real
LaTeX/dvisvgm render has **not** been run on this machine because neither
executable is installed; install both and repeat the job before shipping to a
host that relies on them.
