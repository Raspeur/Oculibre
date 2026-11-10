# Protocol test vectors

Shared by every implementation of [`docs/protocol.typ`](../../../docs/protocol.typ) (§16): the Rust
crate `desktop/protocol` and the Kotlin code of the Android app must both pass them.

**Generated, do not edit by hand:** `cargo run -p oculibre-protocol --example gen_vectors` (from
`desktop/`). The Rust test `vectors` fails when these files are stale.

## Files

- `<name>.hex`: the exact bytes, lowercase hex, one line.
- `<name>.json`: what a decoder must find, with a `kind`:

| `kind` | Fields | What to check |
|---|---|---|
| `preamble` | `major`, `minor`, `role` (`camera` / `host`) | decode the 8 bytes; re-encoding gives the same bytes |
| `message` | `type` (name), `header` (`type`, `flags`, `stream`, `length`, `timestamp`), `fields` | read one message from the bytes; compare `header` and `fields` |
| `error` | `stage` (`preamble` / `message`), `expect_error` | reading must fail with this error kind |

`fields` is:
- for JSON messages (`HELLO`, `BYE`, `STREAM_CONFIG`, `STREAM_END`, `REQUEST`, `RESPONSE`, `EVENT`): the
  JSON object itself. Compare it **semantically** (parsed JSON), not byte by byte: key order may differ
  between JSON libraries. Absent optional keys and defaults must match (see `hello_future_keys`).
- `PING`: `seq`, `t1`; `PONG`: `seq`, `t1`, `t2`, `t3`;
- `VIDEO_FRAME`: `frame_number`, `config_id`, `key`, `discontinuity`, `data_hex`;
- unknown types: `skipped_length` (the payload must be skipped, not decoded).

Error kinds: `bad_magic`, `bad_role`, `too_large`, `json_too_large`, `truncated`, `json`.
`too_large` and `json_too_large` vectors contain **only a header**: implementations must reject them
from the header, before trying to read (or allocate) the payload.
