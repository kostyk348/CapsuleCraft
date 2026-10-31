# Changelog

All notable changes to CapsuleCraft are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions follow [SemVer](https://semver.org/).

## [1.9.0] — 2026-10-31 — Full English, roadmap trio in the native shell

The three native-shell roadmap items land together with a strict-English UI
pass. Native test suite grows from 98 to **135** unit tests.

### Added
- **Native audio playback** (roadmap): audio notes stored as MIME attachments
  play inside the egui shell via `rodio` (WAV/MP3/OGG/FLAC), with Play/Pause/Stop
  and an honest "webm/opus is not decodable offline" note for MediaRecorder
  recordings (`native/src/audio_player.rs`). Playback runs on a dedicated audio
  thread because cpal's output stream is `!Send`.
- **OpenPGP capsule encryption** (roadmap): any capsule exports to an armored
  passphrase-encrypted `.eml.asc` — AES-256, SEIPD v1, iterated+salted S2K via
  `rPGP` — and `.eml.asc` files import back into the vault through the same
  dialog; a wrong passphrase reopens the modal instead of failing silently
  (`native/src/crypto.rs`, `native/src/modals/encrypt.rs`, palette commands
  "Encrypt capsule (.eml.asc)" / "Decrypt .eml.asc into the vault").
- **Offline code execution** (roadmap): JavaScript REPL blocks now run in the
  native shell through the embedded `boa` engine with a console shim and the
  same output layout as the web worker; JSON blocks validate and pretty-print;
  TypeScript/Python/Rust/SQL keep an honest "no interpreter in the
  zero-dependency build" note (`native/src/js_engine.rs`, `blocks/code.rs`).
- **Data-grid formulas** (roadmap): cells starting with `=` are evaluated by the
  same engine — `=SUM(A1:A5)`, `=AVG`, `=MIN`, `=MAX`, `=COUNT`, arithmetic and
  nested cell references, `A1:B3` range expansion; the raw formula is stored,
  the computed value is displayed, click-to-edit shows the source.
- **Animated demo GIF** in both READMEs, rendered from the real app via the
  `--snapshot` CLI (`screenshots/demo.gif`).

### Changed
- **Full English UI with enforced purism**: every block component, the FAQ and
  the onboarding screen in the web build go through i18n; the command palette
  adds Russian search aliases only in the RU locale; Co-Thinker output, ICS
  export labels and numeric formatting follow the detected capsule/UI language.
  A native unit test asserts **no EN string contains a single Cyrillic
  character**, and an engine test asserts Co-Thinker replies to an English
  capsule are Latin-only.
- ICS export language is selectable (`IcsOptions.lang` on native,
  `lang` option in the web `buildIcs`), PRODID carries the locale.
- Native dev profile uses `debug = "line-tables-only"`: full DWARF produced
  multi-GB artifacts that broke constrained CI runners; line tables keep
  backtraces readable.
- CI: the native job now installs `libasound2-dev` + `pkg-config` for rodio.

## [1.8.1] — 2026-10-27 — Release pipeline that does not lose artifacts

A patch release. No application code changed: this ships the rebuilt release
pipeline from PR #13 and republishes v1.8.0's artifacts, which were lost to a
race condition. The app you get is v1.8.0.

### Added
- **Native Windows portable build** (PR #13): `CapsuleCraft_native_<version>_x64.zip` — the egui shell compiled on `windows-latest`, no WebView2, no installer. Windows users no longer get only the legacy Tauri build.
- **Native test job in CI** (PR #13): `cargo check` plus the full `cargo test -j 1` suite for `native/` on every pull request and push to `main`.

### Fixed
- **Artifacts no longer race the release**: `create-release` runs first and guarantees the GitHub Release exists, so the fast native jobs (`native-appimage`, `native-macos`, `native-windows`) can upload instead of failing on a 404. This is why v1.8.0 shipped without its AppImage and macOS bundle.
- **One authoritative `SHA256SUMS.txt`**: a single `finalize-release` job downloads every asset, hashes all of them once, uploads the file and then publishes the draft. Previously each job overwrote the checksums with its own partial list.
- **macOS Tauri job no longer fails on upload**: the artifact step uses `if-no-files-found: warn` and covers `.dmg`, so the missing `*.msi`/`*.deb` masks stop killing the run.
- **AppStream metadata is no longer hardcoded** (introduced in PR #13): the generated `appdata.xml` takes `<release version=... date=...>` from the tag and the build date instead of a frozen `1.8.0 / 2026-10-26`.
- **An incomplete release can no longer be published**: `finalize-release` fails when any build job did not succeed and leaves the release as a draft, instead of publishing a half-empty release (as happened with v1.8.0).
- **Version drift in the manifests**: `src-tauri/Cargo.toml` still said `1.5.0` and `package-lock.json` said `1.4.0`; both now match `package.json`, `tauri.conf.json` and `native/Cargo.toml`.

## [1.8.0] — 2026-10-26 — Two-tier header, Zen mode, macOS builds

### Added
- **Native two-tier header and Zen focus mode** (PR #12 by [@kobaltgit](https://github.com/kobaltgit)): a brand bar plus section tabs (Page / Planning / Graph / Threads / MIME / Email), Standard vs Zen layout, a wide canvas mode for the editor, and persistent UI state (`native/src/ui_state.rs`) so the shell reopens the way you left it.
- **macOS arm64 builds in CI**: an ad-hoc-signed native `CapsuleCraft_native_*_arm64.app.zip` (egui shell) plus a legacy Tauri `.dmg`. `SHA256SUMS-macos.txt` is published next to them.
- **AppImage hardening**: built on Ubuntu 22.04 (glibc 2.35), bundles the X11/xcb/xkbcommon/wayland client libraries that winit loads through `dlopen`, ships AppStream metadata (`appdata.xml`), and CI fails the build if the AppDir contains broken symlinks.
- **Contributors** section in both READMEs.

### Fixed
- **IMAP settings modal**: the four buttons no longer overflow the 520 px panel — *Test connection* / *Sync now* and *Close* / *Save* now sit on two rows.
- **AppImage no longer dies in minimal sandboxes** (`libxkbcommon-x11.so could not be loaded`): `cp -a` had copied library symlinks without their targets. The AppImageHub catalog now reports **Test passed**.

## [1.7.0] — 2026-10-25 — Built-in FAQ, no demo content

### Added
- **Built-in FAQ modal in the native shell**: eight honest answers about storage, format, search, threads, transfer and import. Opened from the top bar (`ⓘ FAQ`), `F1`, the command palette, the empty-vault screen, and `--view faq` for headless capture. RU/EN.
- **No demo content anywhere**: the generated seed capsules were removed from the repository (`scripts/seed_vault.mjs` stays as an explicit dev tool, its output `vault/` is now gitignored). First launch is an empty vault plus the FAQ — nothing pretends to be your notes.
- **Native FAQ screenshot** (`capsulecraft-native-faq.png`) generated by the app itself.

### Changed
- **README rewritten** (EN + RU): a single visual set (native screenshots only), a feature-first structure, a comparison table vs Obsidian / OneNote / Notion and an explicit strengths section.
- Native cargo test suite: **98 unit tests** (added `--view faq` parsing tests).

## [1.6.0] — 2026-10-25 — Native standalone AppImage

### Added
- **Native standalone AppImage** (`CapsuleCraft_native_1.6.0-x86_64.AppImage`, ~8 MB): the egui shell packaged with linuxdeploy with **zero bundled libraries** — the binary depends only on `libc` / `libm` / `libgcc_s` (present on every Linux with glibc ≥ 2.17) plus an X11 or Wayland desktop. No WebKitGTK, no WebView2, no runtime package installs.
- `SHA256SUMS.txt` published with the artifact.

### Changed
- **Related pages by meaning** (from master evolution): the page sidebar ranks capsules by tf-idf cosine similarity over the existing BM25 index, and the page tree gained hierarchy drag & drop.
- Native cargo test suite grew to **97 unit tests** (from 69 at v1.5.0).
- README installers table now separates the **native** AppImage (no webview) from the legacy Tauri/web AppImage (which still needs `libwebkit2gtk-4.1-0`).

## [1.5.0] — 2026-10-23 — CapsuleCraft

### Changed
- **The project is renamed CapsuleOS → CapsuleCraft.** The new name appears in the app title, README (English and Russian), docs, workflows and user-facing strings, with a mascot and a matching icon set: Windows `.ico` (16–256), macOS `.icns`, Linux PNGs, Microsoft store tiles and the web favicon. The native egui shell draws the mascot in its title bar and as the taskbar icon.
- **Compatibility is deliberate**, so nothing on disk changes: the `.eml` format, `X-Capsule-*` headers, the `@capsuleos.local` Message-ID domain, the app-data folder `com.capsuleos.app`, npm/Rust crate names and existing browser storage keys all stay. Every vault written by CapsuleOS opens as-is.
- Historical screenshots in the README still show the previous name; they are labelled as pre-rename captures, and the current native screenshots are generated by the app itself.

### Added
- **Native egui prototype (`native/`)**: a webview-free shell over the same `.eml` vault. This release makes it build and run against egui 0.37 and adds the first features on top of the ported engines:
  - **`[[Wiki links]]`** — new `links` engine (parse spans, resolve by subject or Message-ID, list backlinks) with tests; paragraphs paint links in the accent colour, the document lists **Links** and **Linked from**, and clicking a target navigates or creates the missing page with that title.
  - **Renaming keeps the wiki alive** — when a page title changes, every `[[old title]]` in the vault is rewritten (imported emails are never touched), with a toast reporting how many links and pages were updated.
  - **BM25 sidebar search** — ranks whole-page text and shows the score plus a one-line excerpt centred on the first hit (`search::snippet`, Cyrillic case folding), with the matched words highlighted; falls back to the parent path when the match came from a tag.
  - **Folder watching** — the native shell reloads when an `.eml` changes on disk, debounced and never while there are unsaved edits (its own writes are ignored), with a "folder watched" indicator in the sidebar.
  - **Drop-to-import** — dropping `.eml` files on the window imports them byte-for-byte, with a dimmed drop hint and a skipped-file count; the file dialog shares the same code path.
  - **`--snapshot`** — renders the real window to PNG (`--view`, `--page`, `--search`, `--scroll`, `--locale`, `--frames`), so README screenshots come from the app itself.
  - **Threads view fixes** — cycles are walked once, orphan replies stay visible, replies render under their root instead of inside a 44 px row, and a Threads / All pages switch with real counts replaces the stray empty-state label.
  - **Graph labels, planning toolbar and plurals** — nodes carry their titles, stat boxes no longer collide with the mode switch, and Russian plurals use three forms (`1 блок / 2 блока / 5 блоков`).
- **External email parser & dual preview modes** (`engine/emailParser.ts`): allows dropping raw `.eml` emails from Gmail, Outlook, Apple Mail, Thunderbird or newsletters directly into the vault folder.
  - Recursively resolves MIME multiparts (`multipart/mixed`, `multipart/related`, `multipart/alternative`) and single-part emails.
  - Decodes Quoted-Printable (RFC 2045) and encoded-word headers (RFC 2047 `B` & `Q` encodings with adjacent whitespace collapsing per Sec 6.2).
  - Handles `Content-Transfer-Encoding: base64` for HTML/text bodies.
  - Robust boundary parameter extraction and regex escaping.
  - Full HTML entity decoding (`&copy;`, `&reg;`, `&trade;`, `&mdash;`, etc.) and automatic stripping of invisible preheader padding hacks (`&#847;`, `\u034F`, `\u200B`, `&zwnj;`).
  - Converts email HTML into clean CapsuleOS note blocks (headings clamped to 1..3, tasks list, callout quotes, paragraphs) without duplicate content from layout `div`s.
  - Resolves inline `cid:` images to Data URIs and preserves raw Base64 for binary attachments.
  - **Dual preview modes** in `AutonomousHtmlPreview.tsx`: segmented toggle between **«Original Email»** (authentic email styling, white background, CID images, sandboxed) and **«Capsule Dashboard»** (dark theme block note view), plus contextual `.html` download.
  - 24 parser tests in `emailParser.test.ts`; the suite is now **151 tests / 11 files**.
- **Live vault folder watcher** (PR #6): desktop builds watch the `.eml` folder with a debounce and a write-loop guard, so files edited outside CapsuleOS appear without a restart.
- **Hierarchical pages (wiki tree)**: pages can be nested without touching mail threading — `parentId` is persisted in a `X-Capsule-Page-Parent` MIME header. The sidebar is now a collapsible page tree (search stays a flat BM25 relevance list), the editor shows a clickable breadcrumb, a parent picker, and a "Subpage" action. Reparenting is cycle-safe (a page cannot become its own descendant), dangling parents are promoted to roots, and deleting a page never cascades — its children become roots. The graph draws hierarchy as amber `Sub-page` edges. 8 hierarchy tests + MIME round-trip coverage.
- **Cross-project task filters**: one filter bar drives Agenda, Kanban, Gantt and the dependency editor — free-text search (task text, capsule subject, list title, tags), status (`all / open / todo / doing / done`), priority, due bucket (`overdue / today / next 7 days / no date`), capsule and tag. The shown/total counter and stats reflect the filtered set, calendar dots and ICS export follow the visible rows, and a new task is never silently hidden by a stale filter (the filters clear after quick-add). Pure `filterTasks` engine + 5 tests.

### Fixed
- Imported email text keeps inline formatting tight (`<strong>AI for Everyone</strong>.` → `AI for Everyone.` instead of `Everyone .`), astral-plane numeric entities (`&#128512;`) decode to one code point, and quoted blocks no longer ship a hardcoded Russian "Цитата" title — the editor shows an empty title, the exported dashboard falls back to "Quote".

### Changed
- **Zero demo content in block defaults.** Inserting a DataGrid no longer plants a `North/South/East` sample CSV or a Russian chart title: the block is created with an empty attachment and shows an explicit empty state with a real CSV file picker. A whiteboard starts with an empty scene instead of pre-placed "Cognitive Node A" shapes. New capsules and replies start with a single empty paragraph — no "Новая мысль" heading, no instructional filler, no auto-tags. Removed the unused `createChildReply()` path that injected canned Russian "AI" criticism. Known gaps are now tracked in [LIMITATIONS.md](LIMITATIONS.md).

## [1.4.0] — 2026-10-22

### Added
- **Planning layer** (`engine/planning.ts`): aggregates every TODO block across the vault into one task list, with local `YYYY-MM-DD` day keys (DST-safe), month grid, overdue / today / week buckets, priority sorting/filtering, and RFC 5545 **VTODO** ICS export. New **Planning** tab + 21 engine tests.
- **Kanban board** in the Planning view: fixed To do / In progress / Done columns, drag-and-drop between columns, per-card status selector, and status kept in sync with the checkbox. ICS export now maps `doing` → `IN-PROCESS`, `done` → `COMPLETED`.
- **Gantt timeline** in the Planning view: day-scale chart with week labels, today marker, bars from `start` to `due`, amber styling + lock icon for tasks whose prerequisites are unfinished, and SVG dependency arrows. The editor below the chart sets start/due dates and a "depends on" prerequisite; candidate lists are filtered by `wouldCreateCycle`, so a dependency cycle cannot be created. Task identity is stable across the vault (`blockId:itemId`), and dependencies are stored as `blockedBy: string[]` inside the TODO item.
- **`/task`**: typing `/task` inside a paragraph turns that paragraph into a trackable todo block (one task per line); selecting it with no text inserts an empty task. The slash menu is now fully RU/EN localized.
- **Command palette** (`Ctrl/Cmd+K`): every action searchable and ranked (acronym + fuzzy scoring), plus capsule navigation — 12 tests.
- **Undo/redo** for capsule edits (`engine/history.ts`): coalesced typing bursts, per-capsule stacks, palette + keyboard shortcuts.
- **RU / EN localization**: dependency-free i18n store, typed dictionaries, persistent locale toggle (`localStorage`), Russian source dictionary, English translation (some block-level labels still pending).
- **Task properties in TODO blocks**: due date and priority rendered/edited inline, feeding the planning layer.
- **Graph UX**: auto-fit on load, selected-node styling, responsive navbar with the Planning tab.

### Changed
- **Audio notes are real or explicit — never simulated.** Adding a voice note starts `MediaRecorder` capture immediately; if the microphone is unavailable or denied, the block offers two honest actions: record again or attach an audio file. Removed the default fake waveform, fake playback timer, `0:30` default duration, the `SIMULATED` badge and the "simulated" MIME label. Attached/recorded audio stays a real base64 MIME part of the `.eml`.
- **No demo content anywhere**: deleted `INITIAL_CAPSULES`, `resetToStarterVault()`, the "load sample vault" button, palette action, toast and localization keys. Fresh installs start with a genuinely empty vault; new capsules are created with an empty body paragraph instead of placeholder demo text.
- **Co-Think is model-agnostic**: removed Claude / Gemini / DeepSeek / GPT mentions from the modal, defaults, block authorship, thread classification and tests. The engine is labeled `Local Co-Thinker · no external model`; model selection returns when a real integration exists.
- Sidebar card now says "Local-first" and describes the actual `.eml` folder behaviour instead of claiming autonomous sync.

### Fixed
- Escape closes modals; AI block upserts no longer duplicate on repeated critique; graph selection no longer restarts the force simulation.
- Version badge and docs now report the real test count (109 tests / 8 files).

## [1.3.0] — 2026-10-22

### Fixed
- **Offline Co-Thinker was canned — now it actually reads the capsule.** The previous "AI" returned hardcoded text about LTV/CAC no matter what the capsule contained (even `data_insight` invented "Средний CAC: $117.50" out of thin air). The rewritten engine (`offlineAi.ts`) is content-aware and honest: it mines the capsule's real numeric claims and quotes them, extracts actual keyword themes (RU+EN stopwords filtered), runs rule-based risk scans (absolute claims, missing downside, thin extrapolation, deadline pressure), computes **real statistics from attached CSVs** (mean/min/max, trend by OLS slope, first→last delta %, Pearson correlation with a "correlation ≠ causation" caveat) and **refuses to invent numbers** when there is no data. Fully deterministic: same capsule → same analysis; personas (Claude/Gemini/DeepSeek) change the framing and honest confidence, not the facts.

### Added
- **"Похожие по смыслу" (related capsules)**: new `relatedCapsules()` in the vault store ranks every other capsule by **tf-idf cosine similarity** over the same inverted index that powers BM25 search (weight = idf × (1 + ln tf), SMART-style). The editor's links panel gained a third section with similarity-percentage badges; direct backlink targets are excluded (already listed). No LLM, no embeddings — just the index.
- `Bm25Index.similarity(a, b)` — tf-idf cosine over indexed documents.
- Engine test suite grown from 30 to **57 tests**: CSV parsing/math (incl. the demo dataset's real CAC mean 106.67 and CAC↔LTV correlation < −0.95), claim mining, risk rules, keyword extraction, anti-canned regression ("two different capsules → two different analyses"), determinism, related-capsule similarity.

## [1.2.0] — 2026-10-22

### Added
- **Okapi BM25 relevance ranking** for full-text search: the vault now scores results instead of listing "anything that matches". Ranking combines classic BM25 (IDF-weighted terms, term-frequency saturation, document-length normalization) with prefix-term matching for autocomplete-style queries and the legacy subject/tag substring boosts; ties break to the newer capsule. The sidebar shows the match count ("N matches · BM25 ranked"), the indexed-term count and a per-capsule relevance badge (Σ-score + matched terms on hover). `[[wikilink]]` autocomplete inherits the same ranking.
- **Linux builds in the release pipeline**: tagged releases now also produce `CapsuleOS_*_amd64.AppImage` (portable) and `CapsuleOS_*_amd64.deb` (Debian/Ubuntu, built on Ubuntu 22.04 / glibc 2.35+) alongside the Windows MSI/NSIS installers, with per-OS SHA256SUMS artifacts. Release workflow is a two-platform matrix with fail-fast disabled.

### Changed
- Engine test suite grown from 17 to 30 tests (BM25 tokenizer, IDF, tf-saturation, length normalization, prefix matching, corpus bookkeeping).

## [1.1.0] — 2026-10-22

### Fixed
- `parseEml` no longer leaks the internal `canvas_state.json` part into the attachment list of every imported capsule; attachments with boundary-like or non-Latin1 payloads now round-trip via `Content-Transfer-Encoding: base64`.
- MIME boundaries are deterministic (FNV hash) — re-assembling an unchanged capsule produces byte-identical `.eml`, so the folder vault stops rewriting files on every save.
- Co-Think modal modes are wired to the offline engine (`synthesis` → AI Synthesis callout, `counter_arg` → child `.eml`, `data_insight` → Data Insight callout); reply capsules reuse the AI-generated blocks instead of a static template.
- Slash-command menu opens only at line starts / after whitespace — "and/or" and URLs no longer trigger it, and the trigger `/` is never left in the text.
- Code REPL: JavaScript runs in a sandboxed Web Worker with a 3s watchdog (infinite loops no longer freeze the app); JSON mode validates and pretty-prints; Python/Rust honestly report that no offline interpreter is bundled instead of failing with JavaScript syntax errors.
- Tag filter chips rendered a double hash (`##finance`); `Cmd/Ctrl+K` now actually focuses the vault search.
- Graph view gained working zoom controls and a deterministic tag-color palette; Thought Threads render callout/todo/audio/code blocks and highlight the selected capsule.
- Deleting a capsule no longer resets the selection unless the deleted capsule was selected; Navbar shows the real version and real index-rebuild time.

### Added
- Real audio for voice notes: attach an audio file — it is stored as a base64 MIME attachment, played back with a click-to-seek waveform, embedded into the autonomous HTML dashboard, and the block badge flips from `SIMULATED` to `REAL AUDIO`.
- In-app microphone recording: the voice-note block records straight from the mic (MediaRecorder + live level meter), saves the take as a real MIME attachment, and uses the recorded envelope as its waveform — voice notes no longer require an external recorder.
- Empty-vault onboarding: fresh installs no longer seed demo notes. A built-in onboarding screen explains the `.eml` concept, answers the frequent questions (FAQ) and offers "New .eml" / "Import .eml" / "Load sample vault"; the FAQ is also reachable anytime from the navbar. The five-capsule sample vault stays available on demand.
- Standalone `.html` dashboard export from the Preview view.
- Honest in-browser benchmark: the modal really pushes N synthetic capsules through `parseEml` + graph build (previously it simulated 'SIMD/AVX2 mmap' numbers).
- Vitest engine test suite (17 tests) and a `test` step in CI.
- Bilingual documentation: `README.md` (English) and `README.ru.md` (Russian) with a full screenshot tour.
- `CONTRIBUTING.md`, `CHANGELOG.md`, issue and PR templates.

## [1.0.0] — 2026-10-21

### Added
- Block canvas editor: heading, paragraph, callout, TODO, DataGrid (table/chart/pivot), whiteboard, code REPL, audio note, AI opponent.
- RFC 5322 `.eml` capsules with multipart body: JSON blocks, HTML fallback, MIME attachments addressed by `Content-ID`.
- Vault graph store with inverted index, tag/author indexes and instant full-text search.
- Thought threads, relationship graph, `[[wikilinks]]` with backlinks and a related-capsules panel.
- Markdown live preview and Markdown export (`.eml` stays the source of truth).
- Offline deterministic Co-Thinker (`critique`, `continue`, `reply_thread`, `data_insight`) — no API key.
- Tauri 2 desktop shell: native window, filesystem vault (`vault/` + `vault.json`), native Save As / Open dialogs, zip and `.eml` import/export.
- Raw MIME inspector and autonomous HTML preview.

### Changed
- Vault folder is authoritative on startup; the JSON snapshot is only a cache.

### Infrastructure
- GitHub Actions: Windows x64 release pipeline (MSI + NSIS, `SHA256SUMS.txt`) on tags and manual dispatch.
- CI: TypeScript typecheck, Vite build, `cargo check`, `cargo fmt --check`, `cargo clippy -D warnings`.

[Unreleased]: https://github.com/kostyk348/CapsuleCraft/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/kostyk348/CapsuleCraft/releases/tag/v1.1.0
[1.0.0]: https://github.com/kostyk348/CapsuleCraft/releases/tag/v1.0.0
