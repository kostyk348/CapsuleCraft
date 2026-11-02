# CapsuleCraft v1.12.2 — EN promo copy

Project: https://github.com/kostyk348/CapsuleCraft
Releases: https://github.com/kostyk348/CapsuleCraft/releases
Attach: banner_x_1200x675_en.png (first tweet / Reddit), demo.gif (tweet 3 / Reddit body).

---

## 1. X / Twitter thread (EN) — 4 tweets, attach banner to tweet 1, demo.gif to tweet 3

**1/**
Your knowledge base should outlive the app you wrote it in — and your AI shouldn't live in someone else's cloud.

CapsuleCraft is a local-first notes & planning app where every page is a plain RFC 5322 `.eml` file on disk. No database. No cloud. No accounts.

v1.12.2 out now ⬇ [image]
github.com/kostyk348/CapsuleCraft

**2/**
What's new in v1.12 — an ⚙ Integrations catalog: offline engines that appear in the app only after YOU download them. Nothing bundled, nothing phones home:

■ OCR — Xiaomi-OCR-0 on the pure-Rust mistral.rs engine (✨ on any image → text lands in the note)
■ STT — whisper running in-process (candle, CPU): "Transcribe recording" under any audio block
■ TTS — Russian voices (Ирина / Дмитрий) via sherpa-onnx, 🔊 reads the page aloud
■ Native PDF & image viewers

**3/**
The storage bet is unchanged: one page = one .eml.

— open notes in Thunderbird/mutt
— `grep` the whole vault
— `git diff` your thinking
— sync with Syncthing/rsync
— installing AI components never touches the vault folder

Delete the app tomorrow — the data won't notice. [video: demo.gif]

**4/**
Also inside (all offline, all local):

■ block editor + [[wiki links]] + live backlinks
■ BM25 search w/ scores & snippets (Cyrillic-aware)
■ Agenda / Kanban / Gantt + ICS export
■ mic recording → transcript → note
■ OpenPGP capsule encryption
■ offline Co-Thinker + MCP server for agents

Native Rust + egui shell, ~11 MB AppImage. MIT.
⬇ github.com/kostyk348/CapsuleCraft/releases

---

## 2. Reddit show-post (EN)

**Subreddits:** r/PKMS (primary), cross-post to r/opensource, r/rust, r/selfhosted, r/NoteTaking
**Flair:** [Show / Self-Promotion] where required — read each sub's self-promo rules first.

**Title:**
I built a PKM where the database is a folder of .eml files — and v1.12 adds local AI (OCR, speech-to-text, TTS) you download from an in-app catalog. Rust/egui, MIT, fully offline.

**Body:**

Hey r/PKMS! For the past months I've been building **CapsuleCraft** — a local-first note editor & planner whose storage format can't lock you in: **one page = one plain RFC 5322 email file (.eml)** on disk. The new v1.12.2 adds the thing people kept asking about: **local AI that doesn't come from a subscription.**

**Why .eml?** Because "your data outlives the app" shouldn't be a marketing claim. A vault is just a folder: Thunderbird opens every note, `grep` searches it, `git` diffs it, `rsync`/Syncthing syncs it. There is no database underneath — no SQLite, nothing. No migrations, no exports: the file *is* the export.

**The v1.12 headline — an ⚙ Integrations catalog.** Offline engines appear in the app only after you explicitly download them. Nothing is bundled; sizes are shown up front; components install into the app-data directory, so **the vault folder stays a pure folder of .eml files**:

- **OCR** — Xiaomi-OCR-0 (0.8B VLM) served by the pure-Rust mistral.rs engine (no llama.cpp, no Python). Hit ✨ on any image attachment → recognized text is appended to the note.
- **Speech-to-text** — whisper tiny/base/small run in-process on candle (pure Rust, CPU); audio is decoded by a built-in RIFF/WAVE parser; Russian is supported. "Transcribe recording" sits right under the audio block.
- **Text-to-speech** — piper VITS Russian voices (Ирина / Дмитрий) through sherpa-onnx; a 🔊 button reads the page aloud via the built-in rodio player.
- **Native PDF & image viewers** — pdfium for PDFs (navigation + zoom), `image` crate for PNG/JPEG/GIF/WebP.

All of it works with zero network calls after download. No API keys, no telemetry, no accounts.

**What's inside (the core, unchanged since the start):**
- **Block editor** — headings, task lists w/ dates & priorities, callouts, code, CSV datagrids (datasets travel as MIME parts addressed by `Content-ID`), audio notes with mic recording (cpal → WAV), whiteboards with undo & marquee select; autosave + strict `.eml` round-trip (if Thunderbird can't open it, the format is wrong)
- **[[Wiki links]]** with a live backlink index; renaming a page rewrites every link in the vault
- **Okapi BM25 search** over the whole vault — relevance scores, highlighted snippets, Cyrillic-aware case folding; "related by meaning" via tf-idf cosine on the same index
- **Planning layer** — every task block aggregated into Agenda / Kanban (drag & drop) / Gantt (dependency arrows, multiple blockers, cycle protection) / calendar, with RFC 5545 ICS export
- **Threads** — replies are real child `.eml`s with `In-Reply-To`/`References`, i.e. standard mail threading
- **OpenPGP capsule encryption** and a JS code REPL with grid formulas (`=SUM(A1:A5)`)
- **Offline Co-Thinker** — deterministic local critique of your drafts (vague words, balance, data anomalies)
- **MCP server** — expose the vault to Claude/Cursor/any MCP client for agent-side search & read
- **Import** — drop `.eml` from Gmail/Outlook/Thunderbird (byte-for-byte), IMAP sync, an Obsidian vault importer
- **Native shell** — Rust + egui, no browser engine: ~11 MB Linux AppImage (zero bundled libraries) plus a .deb, an NSIS setup.exe and portable zip for Windows, and a universal macOS .dmg (Apple Silicon + Intel in one image). RU/EN UI, 182 native unit tests.

**Honest limitations** (tracked in LIMITATIONS.md, not hidden): component downloads don't resume (a 1.75 GB OCR model that fails at 90 % starts over); OCR is CPU-honest — a few seconds per scan, no GPU path in v1.12; the mistral.rs engine is pinned to 0.9.5 and updates with releases; transcribing a long recording occupies one job (the UI stays responsive); TTS is piper-medium quality with fixed speed; ICS export is VTODO-only; no Gantt bar drag-and-drop. It starts empty on purpose — built-in FAQ (F1) instead of fake demo notes.

**Get it:** https://github.com/kostyk348/CapsuleCraft/releases — native installers only: Linux AppImage + .deb, Windows setup.exe (NSIS) + portable zip, macOS universal .dmg; SHA256SUMS included, plus `mistralrs-engine-*.tar.gz` for all three OSes · Source: https://github.com/kostyk348/CapsuleCraft · MIT.

**Demo GIF:** https://github.com/kostyk348/CapsuleCraft/raw/main/assets/promo/demo.gif

Feedback very welcome — especially on the `.eml` format decisions (headers, MIME part layout) and on the integrations-catalog UX (what components would you want next?). PRs open: CONTRIBUTING.md.
