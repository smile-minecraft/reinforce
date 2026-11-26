# Demo fixture

A small, original sample library about phase changes of water. All text, SVG
drawings and metadata here are released under CC0 1.0 (see `LICENSE`).

`bundle.jsonl` is a standard Reinforce proposal batch: each line has an `op` and
`data` field. `reinforce serve --demo` submits this batch as the owner, approves
it with verification promoted, and writes the two SVG files from `assets/` into
the content addressed blob directory. The same folder can be imported with
`reinforce import fixtures/demo --mode owner`, which also publishes both assets.

- `assets/phase-diagram.svg` and `assets/phase-labels.svg` are both 800 × 500,
  so the overlay passes the aspect ratio check for the interactive pins card.
- The exam set declares four questions: single choice, multiple choice,
  any-accepted short answer, and a voided placeholder.
