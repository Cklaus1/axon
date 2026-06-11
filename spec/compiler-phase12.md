# Phase 12: Web UI — Goal Approval Flow

**Status**: In progress
**Depends on**: Phase 9 (replay/audit/sandbox), Phase 10 (CLI surface)
**Roadmap ref**: §4 Phase 12

---

## Overview

Phase 12 ships the **Axon Surface UX** — a web-based goal approval flow backed
entirely by the Phase-10 CLI commands.  Every UI action maps 1:1 to an
`axon foo --json` subprocess call; the server (`crates/axon-web`) is a thin
JSON proxy, not an independent logic layer.

```
User prose
  → POST /api/intent/compile  →  axon intent compile --json
  → POST /api/ast/review       →  axon ast review    --json
  → POST /api/ast/approve      →  axon ast approve
  → POST /api/redteam          →  axon redteam        --json
  → POST /api/deploy           →  axon deploy         --json
  → GET  /api/trace            →  axon trace          --json
```

The typed AST — not the English — is the legal/audit artifact (§2.4).  The web
UI makes the AST visible and requires an explicit user approval before deploy.

---

## Server

`crates/axon-web` — a minimal synchronous HTTP server.

```
axon-web [--port 8080] [--axon PATH]
```

| Flag        | Default       | Description                        |
|-------------|---------------|------------------------------------|
| `--port`    | `8080`        | TCP port to listen on              |
| `--axon`    | `axon`        | Path / name of the axon binary     |

`AXON_BIN` environment variable overrides `--axon` (useful in tests/CI).

---

## API Contract

All endpoints return `Content-Type: application/json`.

Requests that carry file content send `Content-Type: application/json` with a
JSON body.  The server writes the content to a temp file and passes the path
to the CLI.

### `POST /api/intent/compile`

Request body:
```json
{ "content": "# Goal: …\n…" }
```

Runs `axon intent compile --json <tempfile>`.
Response: `axon-intent-compile/1` schema (or `{"error":"…"}` on failure).

### `POST /api/ast/review`

Request body:
```json
{ "content": "fn main() { … }" }
```

Runs `axon ast review --json <tempfile>`.
Response: `axon-ast-review/1` schema.

### `POST /api/ast/approve`

Request body:
```json
{ "content": "fn main() { … }" }
```

Runs `axon ast approve <tempfile>`.
Response: `{ "ok": true, "approved_path": "…" }` or `{"ok": false, "error": "…"}`.

### `POST /api/redteam`

Request body:
```json
{ "content": "fn main() { … }" }
```

Runs `axon redteam --json <tempfile>`.
Response: `axon-redteam/1` schema.

### `POST /api/deploy`

Request body:
```json
{ "content": "fn main() { … }", "risk": "medium" }
```

Runs `axon deploy --json [--risk LEVEL] <tempfile>`.
Response: `axon-deploy/1` schema (extended with `risk`, `stages_run`, `gate`).

### `GET /api/trace`

Runs `axon trace --json`.
Response: trace summary JSON.

---

## UI

Single-page application served at `GET /`.

Layout: 6 vertically stacked panes following the approval-flow sequence.
State flows top → bottom; each step's output appears inline.

| Pane | Title              | Action          | CLI verb              |
|------|--------------------|-----------------|-----------------------|
| 1    | Intent             | Compile Intent  | `intent compile`      |
| 2    | AST Review         | Review AST      | `ast review`          |
| 3    | Approve            | Approve AST     | `ast approve`         |
| 4    | Red Team           | Run Redteam     | `redteam`             |
| 5    | Deploy             | Deploy          | `deploy`              |
| 6    | Trace              | Show Trace      | `trace`               |

---

## Exit Criteria

1. `cargo build -p axon-web` succeeds.
2. `cargo test -p axon-web` passes — server starts, routes correctly, JSON
   responses are well-formed.
3. End-to-end: `axon-web` started; opening `http://localhost:8080` in a
   browser and completing intent → compile → review → approve → deploy for
   `examples/goals/hello-goal.md` produces the same outcome as the
   Phase-10 CLI session in ROADMAP §5.
