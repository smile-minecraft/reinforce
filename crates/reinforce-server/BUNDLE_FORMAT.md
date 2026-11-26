# Reinforce Bundle Format

A bundle is the portable, version-controlled form of a Reinforce batch. It is a
UTF-8 file named `bundle.jsonl`: one JSON object per line, no array wrapper, no
blank-line semantics beyond being ignored. A directory bundle contains
`bundle.jsonl` plus an optional `assets/` directory.

```text
my-bundle/
├── bundle.jsonl
└── assets/
    ├── textbook.pdf
    └── diagram.svg
```

Both `reinforce import <path>` (directory or file) and the
`proposal_submit_batch` MCP tool accept the same operation objects. The server
validates the entire batch before writing anything; a rejected batch returns a
field-level `ValidationReport` and leaves the store untouched.

## Line shape

```json
{"op":"knowledge_card","temp_ref":"$card_1","data":{ ... }}
```

| Field | Type | Rules |
| --- | --- | --- |
| `op` | string | One of the operation names below. |
| `temp_ref` | string, optional | Starts with `$`, followed by letters, digits, `_` or `-`; unique inside the batch. Any string value in any `data` field may be this alias. |
| `data` | object | Complete entity snapshot for the operation. `id` is either a UUIDv7 string or a `temp_ref` alias. |

References are resolved before validation, so an operation may point at an id
generated earlier in the same file:

```jsonl
{"op":"knowledge_card","temp_ref":"$card_1","data":{"id":"$card_1", ... }}
{"op":"note","data":{"id":"01a2...","outline":[{"type":"card_ref","card_id":"$card_1"}], ... }}
```

Unknown aliases, duplicate aliases, and aliases that do not start with `$` are
reported as `TempRef` errors with `op_index`, `temp_ref`, and `field_path`.
Omitting `id` lets the server generate a UUIDv7; for `resource` operations the
existing resource with the same `sha256` is reused when present.

## Operations

Every `data` object is a full entity snapshot, not a patch. Missing required
fields are rejected by domain validation. `data` may also carry any extra
fields understood by a newer core version.

### `resource`

Describes a content-addressed binary file. `sha256` is the lowercase hex digest
of the asset bytes. `kind` is `pdf`, `image`, `audio`, `video`, `transcript`, or
`web`. `sensitivity` is `normal` or `restricted`; `export_policy` is `allow`,
`redact`, or `owner_only`. `page_count`, `width`, `height`, `license`,
`attribution_url`, and `attribution_notice` are optional. `width` and `height`
are required when an interactive-pin overlay needs its aspect ratio checked.

```json
{"op":"resource","temp_ref":"$diagram","data":{"id":"$diagram","kind":"image","title":"diagram.svg","sha256":"22df...","byte_size":983,"page_count":null,"width":800,"height":500,"license":"CC0-1.0","attribution_url":null,"attribution_notice":null,"sensitivity":"normal","export_policy":"allow","created_at":"2026-01-01T00:00:00.000000Z"}}
```

### `anchor`

A source locator on a resource. `anchor_kind` is `pdf_page`, `rect`, or
`media_span`; `rect` is `{x,y,w,h}` in `0..=1`, and `media_span` is
`{start_ms,end_ms}`.

```json
{"op":"anchor","temp_ref":"$anchor_1","data":{"id":"$anchor_1","resource_id":"$diagram","anchor_kind":"rect","pdf_page":null,"printed_page":null,"legend_pdf_page":null,"rect":{"x":0,"y":0,"w":1,"h":1},"media_span":null}}
```

### `knowledge_card`

An atomic card. `blocks` is an array of tagged blocks: `markdown` (`text`),
`math` (`latex`, `display`), `diagram` (`source`, `syntax`, `svg_cache`,
`node_bindings`), `image_occlusion` (`resource_id`, `masks`), and
`interactive_pins` (`resource_id`, `overlay_resource_id`, `pins`). `status` is
`draft`, `staging`, `published`, or `archived`; `verification` is `unchecked` or
`verified`. `topics`, `anchor_ids`, `metadata`, and `unsure_count` complete the
snapshot. Agent submissions are always forced to `unchecked`.

### `note`

An outline tree with `outline` nodes of type `section` (`heading`,
`intro_markdown`, `children`) and `card_ref` (`card_id`). `view_settings` is
reserved JSON.

### `exam_set`

A source-aware container for questions: `source_kind` is `past_exam`,
`question_bank`, `original`, or `custom`; `exam_year` is nullable. When
`is_complete` is true and `expected_questions` is set, the number of attached
question cards must match exactly and `question_no` values must be unique.
Original questions must not claim a past-exam year.

### `question_group`

An optional shared stem for several questions: `exam_set_id`, `title`, and
`shared_stem_blocks`.

### `question_card`

`question_type` is `single_choice`, `multiple_choice`, `short_answer`, `vocab`,
`image_qa`, `image_occlusion`, `pin_identify`, or `pin_locate`. `grading_mode` is
`exact`, `any_accepted`, or `voided`. `options` is a list of
`{key, content, explanation, linked_card_ids}`. `answer` and
`accepted_answers` use option keys or literal values. `answer_source_kind` is
`official`, `peer_note`, `duplicate_question`, `inferred`, `original_authored`,
or `unanswered`; `inferred` needs `answer_basis` and an attached anchor.
`exact` single-choice questions need exactly one answer key.

### `edge`

A typed relation: `edge_type` is `tests`, `confusable_with`, `prerequisite`, or
`variant_of`. `from` and `to` are endpoints tagged with `kind`: `card` (`id`),
`question` (`id`), or `option` (`id`, `key`).

## `assets/` and blob storage

Files in `assets/` are hashed with SHA-256. When a file's digest matches the
`sha256` of a `resource` operation in the bundle, `reinforce import` copies it
into the configured blob directory under `<first two hex chars>/<digest>`.
Unreferenced files are ignored; a bundle without `assets/` imports resource
metadata only. The same layout is used by `reinforce serve` uploads.

## Import modes

- `reinforce import bundle/ --db ./reinforce.db --blobs ./blobs` stages one
  pending proposal as `Agent { name: "cli-import" }`. Verification is forced to
  `unchecked`; an owner reviews it in the triage workbench.
- `reinforce import bundle/ --mode owner` submits as `Owner` and approves in
  the same run, applying every snapshot. Use it only for trusted migrations.
- Any validation failure prints every field error as a JSON line on stderr and
  exits non-zero without writing to the store or blob directory.

## Machine-readable schema

`schema/bundle-line.schema.json` (JSON Schema 2020-12) validates one line. It is
intentionally strict about operation shapes and permissive about extra entity
fields so importers can evolve independently of the core. A complete working
example is in `fixtures/demo/bundle.jsonl`.
