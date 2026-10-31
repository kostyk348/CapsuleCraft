# Known limitations

Honest list of what CapsuleCraft does **not** do yet. Updated as features land — if a
limitation below is fixed, it gets removed instead of being quietly forgotten.

_Last updated: 2026-10-31 (v1.9.0)_

## Page hierarchy

- Siblings are sorted alphabetically; there is no manual drag-to-reorder and no
  persisted `order` field. Renaming a page can move it within its parent.
- The parent link is a custom `X-Capsule-Page-Parent` MIME header. It is a valid
  `X-` header and survives our own round-trip, but external mail clients ignore
  it (they only see `In-Reply-To` / `References`).
- Page hierarchy is deliberately separate from mail threading: sub-pages do not
  appear in the Thread view, and replies do not become sub-pages.
- Deleting a page never cascades — children are promoted to roots. Moving a page
  between vaults (or copying a subtree) is not implemented.
- Reparenting rejects cycles in the engine and hides descendants from the parent
  picker, but existing hand-edited `.eml` files can still contain a cycle; the
  sidebar then promotes one page to a root instead of hiding the branch.

## Planning & tasks

- The Gantt editor edits **one** prerequisite per task. The data model stores
  `blockedBy: string[]` and the chart draws every edge, but the select replaces
  the whole array — extra blockers cannot be added from the UI yet.
- Gantt has no drag-and-drop of bars, no zoom and no rescheduling by dragging;
  dates are edited in the table below the chart.
- Dates are local calendar days (`YYYY-MM-DD`), not instants: there is no time
  of day, timezone or recurring task support.
- ICS export is a VTODO handoff only — no alarms, attendees or timezone block.
- Status model is `todo / doing / done`; no custom columns, WIP limits or swimlanes.
- Completing a blocked task is not prevented — the UI warns, but the action is
  allowed (a warning, not a hard gate).
- Task filters are session-only: there are no saved views, named lists or
  persisted filter sets yet. A reload returns to the unfiltered planning view.
- Filtering is deliberately simple: case-insensitive substring search, one
  status / priority / capsule / tag at a time, and fixed due buckets
  (`overdue`, `today`, next 7 days, no date). No fuzzy search, boolean
  operators, custom date ranges or multi-select facets.
- Quick-add clears the active filters on purpose; without that, a task added to
  Inbox while filtering another capsule would look like a lost write.

## Editor & data

- DataGrid accepts CSV only and reads it from the `.eml` attachment. JSON tables
  and pasting data are not implemented; an empty grid asks for a real CSV file.
- Whiteboards store a flat element list (`rectangle / circle / sticky / arrow /
  text`). There is no grouping, undo, multi-select or connection routing.
- Code REPL executes JavaScript in the browser; other language selectors are
  syntax labels, not real execution environments.
- Grid formula cells evaluate one expression per cell (`=SUM(A1:A5)`,
  `=AVG`, `=MIN`, `=MAX`, `=COUNT`, arithmetic, nested refs). There is no
  cross-cell dependency graph: a cell resolves against the values committed
  in the previous frame, and mutually recursive cells stop after 8 levels.
- New capsules have no default tags or demo body; the subject falls back to a
  localized "New capsule" until you rename it.

## Architecture / product

- Sync, multi-user access and RBAC remain roadmap items. Capsule **export**
  to OpenPGP (`passphrase .eml.asc`) exists since v1.9.0, but there is no
  keyring integration, no per-user public-key workflow and no
  encryption-at-rest for the vault folder itself.
- Native audio playback decodes WAV/MP3/OGG/FLAC through rodio's built-in
  decoders; browser-recorded webm/opus notes are reported as not decodable
  instead of playing silence. There is no seeking, volume or waveform scrub
  yet — just Play/Pause/Stop.
- The native JS engine (boa) executes pure computation only: a console shim
  captures `log/warn/error`, but there are no timers, no `fetch`, no filesystem
  access and no shared state between runs.
- No plugin API, webhooks or external API yet. Extensibility is limited to the
  `.eml` file format and the TypeScript engine.
- The web build persists the vault in `localStorage`; the Tauri build persists a
  folder of `.eml` files. Cross-device sync is manual (copy the folder).
- The "Co-Thinker" is a deterministic heuristic engine, not an LLM. It quotes
  real capsule content and refuses to invent numbers, but it cannot reason
  beyond its rule set.
- Imported email HTML is parsed with a dependency-free regex tokenizer, not a
  full HTML5 parser: deeply nested layouts, tables used for design and CSS
  positioning can flatten into fewer blocks.
- The **Original Email** preview runs in a sandbox with scripts disabled. That is
  deliberate — tracking pixels and scripts from real mail do not execute — but it
  also means interactive email HTML (forms, animated widgets) is inert.
- Import covers `.eml` files and vault ZIPs; there is no Evernote/Notion/PDF
  importer. Folder watching is desktop-only; the web build has no file watcher.
- Imported quote blocks carry an empty title by design (no language assumption);
  the exported dashboard labels them "Quote".

## Native egui prototype

The `native/` crate is a separate, experimental shell over the same `.eml`
folder. It is honest about what it does not do yet:

- **Audio playback covers WAV/MP3/OGG/FLAC** (rodio decoders). MediaRecorder
  webm/opus shows an honest "attach mp3/wav/ogg/flac instead" note; there is
  no seeking or volume control yet.
- **Code blocks execute JavaScript and JSON** in the native shell (boa); the
  source of truth is still the `.eml`, and other language selectors remain
  honest syntax labels.
- **Folder watching is debounced and best-effort.** The native shell polls the
  watcher once per frame and waits for unsaved edits to flush before reloading;
  on network shares or filesystems without change notifications it falls back to
  the manual **Reload** button.
- **Search index is rebuilt on save and on navigation**, not incrementally while
  typing. On a very large vault the first search after a bulk import can pause.
- **Links are title-based.** A `[[link]]` matches a page subject (case- and
  whitespace-insensitive) or an explicit `[[<message-id>]]`. Renaming a page
  rewrites title links across the vault, but a link written as a Message-ID keeps
  pointing at the same page (that is the point) and is not rewritten.
- **Backlinks are computed on navigation**, not per keystroke, so a page shows
  the backlinks it had when you opened it.
- **Imported email is read-only.** The native shell renders `text/plain` /
  `text/html` text and never rewrites a file it did not create.
- **Unsupported blocks are preserved, not edited**: they stay in
  `canvas_state.json` and open in the web app.
- **`--snapshot` is a developer command**: it writes one PNG and exits; it is not
  a live screencast.

