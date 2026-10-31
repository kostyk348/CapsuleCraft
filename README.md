<div align="center">

<img src="screenshots/capsulecraft-mascot.png" alt="CapsuleCraft — a little mascot holding an .eml capsule" width="180">

# CapsuleCraft

### Your knowledge base is a folder of emails.

**A local-first note editor and planner with no database, no cloud and no accounts —
every note, task and dataset is a plain, RFC 5322 `.eml` file on your disk.**

[![Release](https://img.shields.io/github/v/release/kostyk348/CapsuleCraft?sort=semver&color=0ea5e9&label=release)](https://github.com/kostyk348/CapsuleCraft/releases)
[![CI](https://github.com/kostyk348/CapsuleCraft/actions/workflows/ci.yml/badge.svg)](https://github.com/kostyk348/CapsuleCraft/actions/workflows/ci.yml)
[![native tests](https://img.shields.io/badge/native%20tests-135%20passing-22c55e)](native/)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![No DB](https://img.shields.io/badge/database-none-orange)](#why-eml)
[![Offline](https://img.shields.io/badge/offline--first-success)](#strengths)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen)](CONTRIBUTING.md)
[![Listed on Remade](https://remade.wiki/badges/listed-on-remade.svg)](https://remade.wiki/projects/capsulecraft/)

[English](README.md) · **Русский**

</div>

---

![CapsuleCraft demo — page, planning, graph, threads and MIME views](screenshots/demo.gif)

## What is CapsuleCraft?

A desktop app for thinking. Each page is a real email-format file (`.eml` —
RFC 5322 / RFC 2045): open it in Thunderbird, search it with `grep`, diff it in
`git`, back it up with `rsync`, sync it with Syncthing. CapsuleCraft adds the
editor, search, planning and structure on top — and the files stay yours.

- 🗂 **Zero database** — a folder of `.eml` files is the entire knowledge base.
- 🛰 **Offline by design** — parsing, search, AI and planning all run locally; the app makes no network calls of its own.
- 🤖 **Offline Co-Thinker** — deterministic local critique and data analysis. No API keys, no cloud.
- 🖥 **Tiny native shell** — Rust + egui, no browser engine. Ships as an AppImage with **zero bundled libraries**.

CapsuleCraft starts **empty** — no demo notes, no filler. A built-in **FAQ**
(top bar `ⓘ FAQ`, `F1`, or the command palette) explains storage, search and
data transfer right inside the app.

## Screenshots (from the real app)

| Page | Search (BM25) |
|---|---|
| ![Native page editor with wiki links](screenshots/capsulecraft-native-page.png) | ![BM25 search with snippets](screenshots/capsulecraft-native-search.png) |
| **Backlinks** | **Planning** |
| ![Wiki links and backlinks](screenshots/capsulecraft-native-backlinks.png) | ![Agenda, kanban, gantt, ICS export](screenshots/capsulecraft-native-planning.png) |
| **FAQ (built-in)** | **Graph** |
| ![Built-in FAQ on a fresh empty vault](screenshots/capsulecraft-native-faq.png) | ![Link graph](screenshots/capsulecraft-native-graph.png) |

<details>
<summary><b>More views</b> — threads, raw MIME, mail preview</summary>

| Threads | MIME inspector |
|---|---|
| ![Reply threads from In-Reply-To](screenshots/capsulecraft-native-threads.png) | ![Raw MIME inspector](screenshots/capsulecraft-native-mime.png) |
| **Mail view** | |
| ![A page as a real email](screenshots/capsulecraft-native-mail.png) | |

</details>

## Features

- **Block editor** — headings, paragraphs, task lists, callouts, code blocks,
  CSV datagrids, audio-attachment notes; autosave, undo/redo, `.eml` round-trip.
- **`[[Wiki links]]`** — parsed from page text, clickable, with a live backlink
  index («Linked from»); renaming a page rewrites every link.
- **Search** — Okapi BM25 over the whole vault with relevance scores, highlighted
  snippets and Cyrillic-aware case folding; «Related by meaning» ranks pages by
  tf-idf cosine similarity using the same index.
- **Planning** — every task block across the vault aggregated into Agenda,
  Kanban, Gantt (with dependency arrows and cycle protection) and calendar, with
  RFC 5545 ICS export.
- **Threads** — replies are real child `.eml`s with `In-Reply-To`/`References`,
  exactly like email. Your reasoning tree is standard mail threading.
- **Datasets** — CSV/JSON travel as MIME parts addressed by `Content-ID`;
  datagrid blocks render them as tables — and `=SUM(A1:A5)`-style formula cells
  are evaluated by the embedded offline JS engine, both in the browser and in
  the native shell.
- **Native audio playback** — audio notes stored as MIME parts play right in
  the egui shell (rodio: WAV/MP3/OGG/FLAC) with play/pause/stop controls.
- **OpenPGP capsule encryption** — any capsule can be exported as an armored,
  passphrase-encrypted `.eml.asc` (AES-256, SEIPD v1 via rPGP) and imported
  back with the same dialog; fully offline, no gpg binary required.
- **Offline code execution** — JavaScript REPL blocks run in the native shell
  through the embedded boa engine (the web build uses a sandboxed worker);
  JSON blocks validate and pretty-print; Python/Rust stay honest about having
  no interpreter in a zero-dependency build.
- **Import** — drop `.eml` files into the window (byte-for-byte), import from
  IMAP or migrate an existing Obsidian vault (`--import-obsidian`).
- **AI-agent ready** — expose the vault over [MCP](https://modelcontextprotocol.io)
  so Claude, Cursor or any MCP client can read and search your knowledge base.
  The built-in Co-Thinker argues with your theses offline instead.
- **Built-in FAQ + snapshot CLI** — `--snapshot` renders the real window to PNG
  (which is how every screenshot above was made). RU/EN interface included,
  and the EN locale is enforced 100% Latin by a unit test.

## Why `.eml`?

Most apps lock your knowledge in a database that dies with the app. CapsuleCraft
flips it: one page = one plain-text message on disk.

| | Typical PKM app | CapsuleCraft |
|---|---|---|
| Storage | closed database | plain `.eml` files |
| Opened by | the app only | any mail client, `grep`, `git` |
| Export | lossy conversion | the file is already the export |
| Migrations | App updates | never needed |
| Telemetry / accounts | varies | none — the folder is the app |

## Capsule format

```eml
Message-ID: <capsule-1755000000000@vault.local>
Subject: Q4 pricing experiment
From: Owner <owner@vault.local>
To: Vault <vault@capsuleos.local>
X-Capsule-Tags: pricing, experiment

--capsule-boundary
Content-Type: application/json; name="blocks.json"
Content-ID: <blocks_json>

[{"id":"blk_1","type":"heading","data":{"level":1,"text":"Q4 pricing experiment"}}]
--capsule-boundary
Content-Type: text/csv; name="cohorts.csv"
Content-ID: <dataset_cohorts_csv>

cohort,users,conversion
Q4-A,1200,0.041
--capsule-boundary--
```

The JSON part is the structured block canvas; extra parts are datasets and
attachments addressed by `Content-ID` from datagrid blocks. Threads ride on
standard mail headers.

## How CapsuleCraft compares

| | **CapsuleCraft** | Obsidian | OneNote | Notion |
|---|---|---|---|---|
| Storage format | plain `.eml` (open standard) | Markdown + hidden app data | proprietary binary sections | proprietary cloud database |
| Works fully offline | ✅ by design | ✅ | ⚠️ mostly | ⚠️ limited |
| Account required | ❌ never | ❌ (paid sync needs one) | ✅ Microsoft account | ✅ for sync |
| Your data readable without the app | ✅ Thunderbird / mutt / `grep` | ✅ Markdown | ❌ | ❌ (export only) |
| `git`-diffable, `grep`, `rsync` | ✅ native | ✅ | ❌ | ❌ |
| Real email threading | ✅ `In-Reply-To`/`References` | ❌ | ❌ | ❌ |
| Local deterministic AI | ✅ Co-Thinker | ⚠️ via plugins/cloud | ⚠️ Copilot (cloud) | ⚠️ cloud |
| Open source | ✅ MIT | ❌ (free but closed) | ❌ | ❌ |
| Install size (Linux) | ~11 MB AppImage, no webview | ~100 MB Electron | 100+ MB | web app |
| Vendor lock-in | none | low | high | high |

**Where Obsidian/OneNote win today:** Obsidian's plugin ecosystem, OneNote's
stylus/inking depth, Notion's collaborative cloud editing. CapsuleCraft is
deliberately narrower: local-first storage you can trust for decades.

## Strengths

- **Data outlives the app.** Delete CapsuleCraft tomorrow — Thunderbird still
  opens every note. No migrations, no conversions, ever.
- **Privacy is structural.** No accounts, no telemetry, no API keys, no network
  calls. The AI runs offline because that's the only honest option.
- **Small and fast.** The native shell has no browser engine inside: an
  8–12 MB AppImage, instant start, glibc-only dependencies.
- **Honest launch.** Starts with an empty vault and a built-in FAQ — no demo
  notes pretending to be data. Known gaps are tracked in [LIMITATIONS.md](LIMITATIONS.md),
  not hidden in marketing.
- **A migration path.** An Obsidian importer, IMAP sync and `.eml` import
  get real content in on day one.

## Getting started

### Download

The **native AppImage** (recommended, Linux):

```bash
chmod +x CapsuleCraft_1.9.0_amd64.AppImage
./CapsuleCraft_1.9.0_amd64.AppImage            # default vault...
./CapsuleCraft_1.9.0_amd64.AppImage /path/to/vault   # ...or your folder
```

It needs only glibc, an X11 or Wayland desktop and (for audio playback) ALSA —
no WebKitGTK, no WebView2, nothing else. Every GitHub release also carries
`SHA256SUMS.txt`.

The **native Windows app** (x64) ships as `CapsuleCraft_native_1.9.0_x64.zip`:
unzip it and run `CapsuleCraft.exe`. Standalone native egui shell, zero dependencies,
no WebView2 or installer required.

The **native macOS app** (arm64) ships as `CapsuleCraft_native_1.9.0_arm64.app.zip`:
unzip it, drag `CapsuleCraft.app` into Applications. It is ad-hoc signed; if
Gatekeeper blocks the first launch, run `xattr -d com.apple.quarantine CapsuleCraft.app`.

Legacy web/Tauri builds (Windows MSI / NSIS, Linux deb, macOS dmg) are still
published for parity (on Linux those need `libwebkit2gtk-4.1-0`). The Linux
**AppImage**, Windows **portable zip**, and macOS **.app** are the recommended
native builds — no browser engine, no webview libraries.

> First launch shows an empty vault and the built-in **FAQ**. Press `F1`,
> use the `ⓘ FAQ` button or ⌘K → FAQ. No demo notes are ever created.

### Build from source

```bash
git clone https://github.com/kostyk348/CapsuleCraft.git
cd capsuleos/native
cargo run --release -- /path/to/your/vault
```

Native-only requirements: Rust stable, no GUI toolkit dependencies.

### Snapshot CLI (docs/CI)

```bash
cargo run --release -- --snapshot out.png --page 1 [--view app] [--search "query"] [--scroll 300]
```

## Architecture (native shell)

```text
native/src/
  main.rs        entry, arg parsing, FAQ modal, snapshot CLI
  cli.rs         typed CLI surface
  mail.rs        RFC 5322 / MIME parsing & serialization
  mime.rs        part splitting, Media-Type registry, datasets
  blocks.rs      block tree <-> markdown-ish round trip
  page.rs        page model, ordering, tree
  links.rs       [[wiki links]], backlink index
  graph.rs       capsule relationship DAG
  threads.rs     In-Reply-To / References threading
  planning.rs    agenda / kanban / gantt extraction, ICS export
  search.rs      BM25 index over the whole vault
  mcp.rs         MCP server config for AI agents
  sync_imap.rs   IMAP account sync
  secrets.rs     keyring-backed account storage
  i18n.rs        RU / EN strings
  ...
  views/         page, graph, mail, mime, planning, threads, palette
```

## Tests

```bash
# native shell
cd native
cargo test          # 135 unit tests

# legacy web engine
npm ci
npx vitest run      # 172 tests
```

## Releases

| Version | What | When |
|---|---|---|
| **v1.9.0** | Roadmap trio in the native shell: audio playback (rodio), OpenPGP `.eml.asc` encryption (rPGP), code execution + grid formulas (boa); full English UI with a purism test | 2026-10-31 |
| **v1.8.1** | Release pipeline rebuilt: native Windows portable build, pre-created release (no more lost artifacts), unified `SHA256SUMS.txt`, native test job in CI | 2026-10-27 |
| **v1.8.0** | Two-tier header, Zen focus mode, wide canvas; macOS arm64 builds; AppImage hardening | 2026-10-26 |
| **v1.7.0** | Built-in FAQ in the shell; no demo notes anywhere; README redrawn | 2026-10-25 |
| **v1.6.0** | Native standalone AppImage (zero bundled libraries) | 2026-10-25 |
| **v1.5.0** | Renamed to CapsuleCraft; native egui shell | 2026-10-23 |
| **v1.4.0** | Planning (Agenda/Kanban/Gantt/ICS); email import | 2026-10-22 |
| **v1.3.0** | Co-Thinker offline proposal engine | 2026-09-25 |
| **v1.2.0** | BM25 search; Linux installers | 2026-09-15 |
| **v1.1.0** | Real audio playback | 2026-09-05 |
| **v1.0.0** | First Tauri release | 2026-08-16 |

## Limitations (honest ones)

See [`LIMITATIONS.md`](LIMITATIONS.md). Notably: audio playback covers
WAV/MP3/OGG/FLAC (browser-recorded webm/opus needs conversion), the JS engine
runs no timed side effects or I/O, and vault sync is still a manual folder copy.

## Roadmap

- ✅ v1.0 – v1.9 (current milestone)
- ✅ Native code execution (REPL / JS table formulas) — done in v1.9.0 (boa)
- ✅ Audio playback in the native shell — done in v1.9.0 (rodio)
- ✅ GPG-encrypted capsules — done in v1.9.0 (rPGP, passphrase `.eml.asc`)
- ✅ macOS native builds in CI (arm64)
- ⬜ Optional end-to-end-encrypted vault sync (tamper/censor-proof, still zero-cloud)

## Contributing

PRs welcome — see [`CONTRIBUTING.md`](CONTRIBUTING.md). Changes are tracked in
[`CHANGELOG.md`](CHANGELOG.md).

## Contributors

- **Konstantin** ([@kostyk348](https://github.com/kostyk348)) — creator, architecture, native shell, releases
- **kobaltGIT** ([@kobaltgit](https://github.com/kobaltgit)) — native UI and MIME parser, live vault watching, offline Co-Thinker, MCP server, Obsidian importer, hierarchy drag & drop, IMAP sync; current GUI redesign (two-tier header, Zen focus mode, wide canvas)

## License

MIT — see [`LICENSE`](LICENSE).

<div align="center">

**CapsuleCraft** — your thoughts, stored as mail.

</div>
