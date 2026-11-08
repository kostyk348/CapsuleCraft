# Changelog

All notable changes to CapsuleCraft are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions follow [SemVer](https://semver.org/).

## [2.10.1] — 2026-11-08 — The graph stops overlapping

A critique pass over the v2.10.0 screenshots: circles piled up, labels sat
on top of each other, nodes stacked on the same pixel. Tests 304 → 310.

### Added
- **Bubble packing for the cluster overview**: bubble radii are scaled to an
  area budget (42 % of the canvas, floor 7 px so tiny clusters stay
  clickable) and overlapping bubbles are relaxed apart with an anchor pull
  towards their real centroids, so the folder map stays a map instead of a
  solid blob. The packing is O(k²) per iteration and cached by an input
  fingerprint — it recomputes only when centroids, sizes or the canvas
  change, not per frame.
- **Label de-collision** shared by the pages view and the cluster overview:
  candidates are measured, then kept greedily in priority order (most
  connected first) — a label that would touch an already-placed one is
  skipped instead of drawn over it.
- **Node separation pass** at the end of the layout: every pair of nodes
  closer than a feasible minimum (ideal distance capped by the packing
  budget for n discs in a unit box) is pushed apart, grid-accelerated and
  deterministic; coincident points split along per-node golden-angle
  directions so opposite pushes cannot cancel out. The pages view shows a
  scatter, not a pile.

### Fixed
- Cluster bubbles no longer merge into one blob at any zoom in the overview
  band (60 folders × 500 pages verified under Xvfb).

## [2.10.0] — 2026-11-07 — A graph that survives 200k pages, and Android in the release

The graph view was rebuilt for vault scale: it stays responsive and readable
where the old renderer would have frozen and turned into pixel soup. The
release artifact set gains a signed Android APK. Tests 291 → 304.

### Added
- **Cluster overview LOD** (zoomed far out): one colored bubble per vault
  folder, sized by page count, with the folder→folder edges aggregated and
  weighted; the largest clusters carry labels, and a legend panel lists the
  top clusters (click to fly in). The overview fits the cluster cloud to the
  screen, so it stays readable at any zoom in the band.
- **Density LOD**: when thousands of nodes share the viewport, individual
  dots collapse into screen-space density bins colored by cluster — 200k
  pages render as a legible heat map instead of a stall. Hover names the
  bin's cluster; double-click enters it.
- **Pages LOD**: individual nodes return once the viewport holds few enough
  of them; labels are capped per frame (most-connected first), edges are
  drawn only between visible endpoints with a hard cap.
- **Background layout for big vaults**: above 384 nodes the
  Fruchterman–Reingold relaxation runs on a worker thread (Barnes–Hut
  quadtree, O(n log n) per iteration) and streams snapshots into the canvas
  — the UI paints immediately and sharpens as the layout converges, with a
  progress chip. A vault edit invalidates in-flight snapshots.
- **Layout persistence**: positions are cached by Message-ID in
  `graph_layout.json` next to `ui_state.json`; a relaunch (or a new page)
  starts from the saved layout instead of a bare spiral.
- **Folder clusters + communities**: every page joins its folder's cluster;
  label propagation over the link graph provides deterministic communities
  for flat vaults and the cluster coloring.
- **Signed Android APK in every release** (`CapsuleCraft_<v>_android_arm64.apk`):
  the NativeActivity shell built with cargo-ndk and gradle `assembleRelease`,
  hashed into `SHA256SUMS.txt` and enforced by the finalizer. The repo
  carries its own release keystore (`android/keystore/README.md`) because no
  Actions secret storage is available — the signature is build identity, not
  security, and the job prefers `ANDROID_KEYSTORE_*` secrets when they
  exist. Gradle falls back to the debug key for local builds.
- `--stress-graph N` — swap the vault for N synthetic pages across many
  folders and open the graph: the scale harness for screenshots and smoke
  runs. `--zoom <f>` — initial graph zoom (docs screenshots reach the
  cluster band without scripting wheel events).

### Fixed
- Zooming out no longer turns the graph into overlapping dots: the three
  LOD bands (`graph::pick_mode`, unit-tested) hand over between cluster
  bubbles, density bins and individual pages.
- Repulsion above 384 nodes is Barnes–Hut instead of an O(n²) pass, so a
  200k-page relayout is minutes of background work rather than a frozen UI.
- The screenshot harness runs on `xvfb-run`-free Xvfb again (the QoL pass
  relied on a wrapper that is not in CI).

## [2.9.1] — 2026-11-07 — Nine quality-of-life fixes from a critique pass

A headless critique session (every view screenshotted, every flow read in
code) produced this batch of small, everyday-friction fixes. Tests 286 → 291.

### Fixed
- **The sidebar footer rendered the literal «стр. .eml»** — a broken format
  string shipped with the page counter. It now names the open page:
  `6 .eml · стр. 3 из 6` (EN: `page 3 of 6`), with an em dash when nothing
  is open.
- **The sidebar search placeholder was hardcoded English** ("Search pages")
  even in the Russian locale; the empty-vault message and the «New page»
  button were English-only too. All three are localized now.
- **Planning rows with no date showed a clipped «YYYY-MM…»** placeholder in
  every no-date row. They show a quiet «—» instead, and the full
  `YYYY-MM-DD` format moved to a hover tooltip on the date cell.
- **Graph node labels truncated at 18 characters** («Meeting notes — va…»)
  — they now carry 26 («Meeting notes — vault sync» fits).

### Added
- **Session restore**: the open page (by Message-ID, so vault reordering
  can't break it) and the active view persist to `ui_state.json` and reopen
  on the next launch. The CLI `--page`/`--view` flags still win, and a page
  deleted on disk falls back to the vault root.
- **Word count** next to the block count in the page meta line
  («2 блока · 39 слов»), with proper Russian plural forms (1 слово / 3
  слова / 39 слов). Code fences, item ids and `language` keys don't count.
- **«/» focuses the sidebar search from anywhere** (Gmail-style); if the
  sidebar is collapsed it un-collapses first. Skipped while the palette or
  a text field owns the keyboard.
- **Search clearing**: a ✕ button at the right edge of the search box, and
  Esc clears the query while the field is focused.

## [2.9.0] — 2026-11-07 — Blocks that move, a canvas that navigates, sync that starts

### Fixed
- **Android CI build (E0601)**: the crate was bin-only, but a NativeActivity
  loads the app as a shared library — and the bin target has no `main` under
  `target_os = "android"`. The same `src/main.rs` is now also a `[lib]`
  target with `crate-type = ["cdylib"]`; the Android job builds
  `cargo ndk … --lib` (→ `libcapsuleos_native.so`, the name the manifest
  expects) and the desktop release builds pin `--bin capsuleos-native`.
- **The topmost reorder gap was unreachable** (blocks/mod.rs): when block 0
  is skipped as a duplicate page heading, dragging any block upward
  dead-ended at gap 1 — the pointer over the title area never updated the
  drag target. A pointer lifted above the first rendered row now snaps the
  insertion gap to 0, so «выше всех» finally works on every page.

### Added
- **Block moving, three independent ways** (blocks, document view):
  a **hover toolbar** at the row's top-right — ↑ move, ⧉ duplicate, ↓ move,
  one visible click each; the **grip is a full-height gutter strip** instead
  of a 14×18 dot; **Alt+↑/↓** moves the block the pointer last hovered,
  Notion-style, working even when a popup or the pointer race eats a
  release. **Duplicate** also lives in the grip menu; the copy gets a fresh
  id so todos and links stay independent.
- **Canvas navigation** (blocks/canvas.rs): a reopened canvas with its
  content off the first screen **jumps to the content once** instead of
  showing an empty grid (`canvas_open_view`, tested); keyboard navigation
  while the pointer is over the field — `+`/`=`/`−` zoom, `Home`/`Ctrl+0`
  fit, arrows scroll; the **minimap appears with any content** (it needed
  two nodes before) and its viewport frame is clipped to the map, so a
  panned-away board no longer spills rectangles past the frame.
- **Folder sync between devices** (native/src/sync.rs): two-way merge of
  the vault with any folder another device mirrors — Dropbox, Nextcloud,
  Syncthing, a USB drive. Last-writer-wins per page by mtime with a 2 s
  clock-slack allowance; one-sided pages copy to the side that lacks them.
  The plan is a pure, tested function; applying it is plain file copies.
  UI: «Синхронизировать» in the sidebar footer — plain click syncs with the
  remembered folder, Shift picks a new one; the toast reports
  `+pushed / −pulled`. Five tests cover the plan, the apply and the scan.
- **Android scaffold** (docs/android.md): the same egui binary builds as a
  NativeActivity — `android_main` hands the app-private data dir to the
  vault, `android/` holds a minimal gradle project with no Java code of our
  own, and `.github/workflows/android.yml` builds a debug APK in CI.

### Docs
- **Promo set redone around the real icon** (assets/promo): two banners —
  the Habr one (RU, native-shell focus) and the X/Twitter one (EN,
  icon-forward) — built from the original 1254×1254 Nova icon art
  (`nova_icon_1254.png`, the same icon the README has carried since v2.7.0),
  not from a generated stand-in. The generated neon capsule art and the
  square banner are gone; `scripts/make_promo_v4.py` regenerates the set.

### Tests
- Blocks: a real-pointer toolbar duplicate test, an Alt+arrow keyboard test
  and a gap-0 nudge test join the grip-drag regression (286 total).
- Canvas: `canvas_open_view` fits off-screen content once and never re-fits
  a deliberately moved view.

### Removed
- **The legacy web engine, entirely.** `src/` (React frontend, vitest
  suite), `src-tauri/` (Tauri wrapper, kept only for icons), `index.html`,
  `vite.config.ts`, `tsconfig.json`, `package.json`, `package-lock.json`,
  `public/` and the web co-thinker doc are gone — CapsuleCraft has shipped
  native-only installers since v1.12.1, and the web tree had been dead
  weight ever since. CI now runs the native egui test job only;
  `release.yml` resolves the version from `native/Cargo.toml` and takes
  icons from `native/assets/`; READMEs and CONTRIBUTING describe the
  native-only build.

## [2.8.0] — 2026-11-07 — Clickable plans, a calendar you can use

### Fixed
- **Plans in the planning view never navigated on a plain click**
  («не кликаются планы»): the row background, the calendar day chips, the
  «+N ещё» overflow marker and the Gantt bar were painted-only — clicks
  landed nowhere or entered text editing. Now every plan opens its source
  page when you click it: the row background (widgets on top still win
  their own pointer), a calendar chip, «+N ещё» and the Gantt bar, each
  with a pointing-hand cursor.

### Added
- **Two-pane calendar** (`native/src/views/planning_view.rs`): the month
  grid on the left, the focused day on the right with full task rows and
  a quick add that files the task with that day as the due date (Enter or
  the button, focused on the day field). A hint line explains both moves.
- **Drag a calendar chip onto another day to reschedule it**: press and
  drag a task chip, the hovered day lights up with an accent outline, and
  the drop patches the task's due date with a «Срок перенесён» toast.
  Clicking a day still focuses it; a drop never double-fires as a click.

### Docs
- **Russian README restored**: `README.ru.md` was a v1.x fossil (135-test
  badge, mascot hero). It is now a full bilingual mirror of the current
  README — same structure, same screenshots, v1.0 → v2.8.0 history — and
  the language switcher in both files actually links to the other one
  (the English README pointed «Русский» at itself).
- **Promo banner + social card** (`screenshots/banner.png`,
  `screenshots/social-card.png`): Nova mark, bilingual tagline, feature
  chips; the banner leads both READMEs, the social card is ready for
  GitHub Settings → Social preview.

### Tests
- Planning: five regression tests drive real pointer events through
  egui's pipeline — a row-background click opens the page, a calendar
  chip click opens the page, a chip drag to another day reschedules the
  plan (due patched, toast set, drag state cleared), a day click focuses
  it, and kanban/gantt still draw headless.
- Native suite: 272 → **277** passed (0 failed, 2 ignored).

## [2.7.0] — 2026-11-07 — Four faces and a working drag

### Added
- **Icon sets** (`native/src/brand.rs`): four styles — **Nova** (new
  neon-capsule artwork, the default), **Document** (classic), **Capsule**
  (generated pill mark), **Custom** (any PNG via the file dialog, copied
  next to `ui_state.json`, broken/missing falls back to Nova). Live
  switching: the taskbar icon follows via `ViewportCommand::Icon` without
  a restart. Entries: the titlebar mark cycles all four; the palette
  gains four explicit commands. Legacy `mascot` configs deserialize
  through a hidden alias and normalize to `nova` — no settings resets.

### Fixed
- **Block drag-to-reorder never fired** (announced in v2.2.1, dead since
  birth): the stale-state cleanup ran before the landing check on the
  release frame and wiped the drag state. Cleanup now runs after the
  drag block; a regression test drives a real pointer drag through
  egui's event pipeline and asserts the emitted `Reorder{0,2}` and the
  resulting order (#55).

### Docs
- README: `screenshots/demo.gif` now actually exists — a 14-frame promo
  reel of v2.2 -> v2.7 (it was referenced but never committed); Nova
  hero image; native tests badge 233 -> 272; a Community section; the
  v2.2.1 row admits the reorder op landed in v2.7.0 (#57).

### Tests
- Icon styles: decode sizes, RGBA bytes, custom fallback, custom from
  disk, the four-style cycle, the mascot serde alias.
- Drag reorder: the event-pipeline regression test.
- Native suite: 268 -> **272** passed (0 failed, 2 ignored).

## [2.6.0] — 2026-11-07 — A pen for the board

### Added
- **Freehand drawing on the canvas** (`native/src/blocks/canvas.rs`): a
  «Рисовать» toolbar toggle (new `Icon::Pen`); while on, a drag on the
  empty field collects a decimated world-space stroke instead of a
  marquee. The stroke lands as `{type: "ink"}` — bbox in x/y/width/
  height, points relative to the origin rounded to 0.1; rendering is a
  polyline (a dot for a single click); undo, drag, marquee, fit and
  delete treat it like any element; the pen flag persists like snap.

### Fixed
- De-raced `start_rejects_when_missing_model` (the red native CI on the
  v2.5.0 release merge): `start()` now delegates to a private
  `start_with()` with an injectable worker; the test gates a fake
  worker behind a channel and polls for the failure with a bounded
  wait. 200/200 stressed runs with `-j 1`.

### Tests
- Ink bbox, relative-points round-trip, JSON round-trip, headless draw
  with pen on/off.
- Native suite: 263 -> **268** passed (0 failed, 2 ignored).

## [2.5.0] — 2026-11-07 — Pictures on the board

### Added
- **Image nodes on the canvas** (`native/src/blocks/canvas.rs`): the
  toolbar's «Картинка» lists the pictures already in the capsule plus
  «Выбрать файл…»; the node stores a `cid` reference while the bytes
  stay one-copy in `capsule.attachments` (same scheme as the image
  block, cid derived from the bytes). File picks come back as
  `BlockOp::AttachImage`; both the block-level canvas and page mode
  apply canvas ops now (`AttachImage` upsert, `OpenViewer`).
- Rendering letterboxes into the node rect (`fit_inside`); unresolved
  cids draw a quiet placeholder; double-click opens the full-screen
  viewer.

### Tests
- Factory defaults, cid resolution, letterbox math, headless draw of
  texture and placeholder paths.
- Native suite: 259 -> **263** passed (0 failed, 2 ignored).

## [2.4.1] — 2026-11-07 — The button's result is always on screen

### Fixed
- **Canvas toolbar adds spawn at the visible center** instead of a fixed
  spot near the world origin (`native/src/blocks/canvas.rs`). The viewport
  persists in `data.view`, so after panning — or on a board whose saved
  view sat far from the origin — the new element was added off-screen:
  «при добавлении через кнопку нестабильно показывает». Now it lands at
  `view.screen_to_world(canvas_rect.center())` with a +16 px cascade for
  consecutive adds; the draw pass publishes the canvas rect to egui
  memory and the toolbar reads it back at click time.
- Element factory is `canvas_element_at(kind, at, locale)`; the legacy
  fixed-position wrapper is removed.

### Tests
- Regression: four views × twelve consecutive adds always intersect the
  viewport's world rect; cascade never stacks and never drifts past a
  node size; the rect hand-off is verified by a headless draw pass.
- Native suite: 255 -> **259** passed (0 failed, 2 ignored).

## [2.4.0] — 2026-11-07 — The page becomes the canvas

The promised page-mode switch lands: every page is now **«Страница |
Канвас»** — and sync finally runs like a service, not like a freeze.

### Added
- **Page mode** (`native/src/views/document_view.rs`): a per-page toggle in
  the document header switches between the block editor and one
  full-height canvas driven by the v2.3 engine — pan/zoom, arrows, shapes,
  snap, minimap. The diagram persists inside the same `.eml` as a
  `page-canvas.json` JSON part (the `canvas_state.json` mechanism,
  generalized): no schema changes, no sidecar files, absent on ordinary
  pages, never shown as an attachment.
- **Page nodes** (`native/src/blocks/canvas.rs`): a canvas element that is
  a wiki-link in disguise — accent border, a title you type on it, and
  clicking an already-selected node opens the target page through the
  existing title resolution, offering to create it when the title does not
  exist (the same flow as a clicked `[[link]]`).

### Fixed
- **IMAP sync** (`native/src/sync_imap.rs`): the network round-trip moved
  off the UI thread (v2.3 froze the whole interface mid-sync); downloads
  are duplicate-proof — bodies are content-hashed against the vault and
  downloads are named after the normalized Message-Id with a
  UIDVALIDITY-qualified fallback, so folder resets no longer collide
  silently and already-synced messages are reported honestly
  («+2 downloaded, +1 uploaded, 3 already in sync»); `auto_sync` finally
  works — sync once after startup and every 15 minutes while enabled
  (the flag was stored and never read).
- **Data directories** (`native/src/vault.rs`): `app_data_dir()` never
  returns a relative path again — the old last resort resolved against the
  process CWD, so an AppImage launched from different folders wrote the
  1.75 GB OCR model and the component catalog to a different arbitrary
  place on every launch and re-downloaded next time. New portable
  overrides: `CAPSULECRAFT_DATA_DIR` (data root: components, models, vault
  pointer) and `CAPSULECRAFT_CONFIG_DIR` (settings root). README documents
  both roots.

### Tests
`cargo test` in `native/`: **255 passing** (was 248). New: page-canvas
round-trip (including "not an attachment" and "absent on ordinary
pages"), page-node factory, headless full-page canvas draw, download
naming across folder resets, filesystem-safe names, body digests, and
the data-dir priority chain (override → APPDATA → XDG → HOME → absolute
temp).

## [2.3.0] — 2026-11-07 — The canvas

The board becomes a real diagram tool: an infinite pan/zoom field, arrow
connectors, flowchart shapes, inline text editing, grid snapping and a
minimap — plus a fix for the «уезжающие» buttons in Integrations.

### Added
- **Infinite canvas engine** (`native/src/blocks/canvas.rs`): elements live
  in world coordinates behind a `CanvasView` transform (`screen = world ×
  zoom + offset`). The wheel pans the field and claims the scroll so the
  page stays still; Ctrl+wheel / pinch zoom (0.2×–4×) anchored under the
  cursor, exact even when the limit clamps the gesture. The grid switches
  one ladder step per octave of zoom so its screen pitch stays legible
  (28–57 px). Marquee selection, rubber-band drafting and the minimap all
  live in world space. The viewport persists in `data.view`; a
  fit-to-content button recomputes it from the elements' bounding box.
- **Arrow connectors**: a selected node grows a connector port on its right
  edge; dragging from it drafts an arrow that drops on any other node. An
  arrow stores only `from`/`to`/`label` — endpoints are recomputed from the
  node rects every frame, so moving a node moves its arrows for free, and
  the `.eml` schema stays flat. Connectors exit through the shape border
  (ray-to-rect intersection; diamonds use their exact L1 edge), land with a
  filled arrowhead, and carry an optional midpoint label with a halo.
  Clicking near a segment selects the arrow; deleting a node prunes its
  arrows (`prune_elements`). Undo covers arrow creation, moves and deletes.
- **Inline text editing** — the long-standing gap: canvas text could not be
  edited at all. Double-click on a node (or an arrow) opens an overlay
  `TextEdit` right on the element; focus is requested once, Escape or focus
  loss saves and closes.
- **Flowchart shapes**: diamond (decision) and rounded (terminator/pill)
  join sticky/rectangle/text with their own anchor geometry and rendering,
  plus toolbar buttons («Ромб», «Скруглённый»).
- **Snap to grid**: the «Сетка»/«Snap» toolbar toggle (persisted in
  `data.snap`) makes group drags land on a 20-unit grid — half the zoom-1
  grid pitch, always aligned to the visible lines.
- **Minimap**: bottom-right once two or more nodes exist — content box,
  node rectangles, current viewport outline, click/drag to navigate.
  Arrows are excluded from marquee, bbox, fit and minimap scale (they have
  no rect of their own).
- **Dialog screenshots**: `--modal integrations` opens a modal on startup
  and `--window-size 980x640` targets the app's minimum window, extending
  the deterministic `--snapshot` tooling to dialogs.

### Fixed
- **Integrations**: a long custom-model folder path was printed as an
  unclamped HUD line, stretched its card past the scroll viewport and
  visually dragged the right-aligned «Выбрать папку»/«Убрать» buttons off
  the card. `hud()` now re-layouts with a trailing `..` ellipsis when the
  line does not fit (binary-searched prefix, extra layout pass only on
  overflow) — the same latent overflow closes at every other hud call site
  (requirement lists, sizes, paths, mail headers). The dialog itself is
  clamped to the window, so short viewports can no longer clip its right
  edge where every row keeps its buttons
  (`native/src/widgets/controls.rs`, `native/src/modals/integrations.rs`).

### Tests
`cargo test` in `native/`: **248 passing** (was 233). New: world/screen
round-trip, anchored zoom with clamping, corrupt-view fallback, fit-view
bounds, grid ladder, rect/diamond anchors, edge hit-test with endpoint
clamping, dangling-arrow pruning, arrow-free bbox, topmost-node hit, snap
rounding, hud width clamp, row/button containment under a 140-char path,
`--modal`/`--window-size` parsing. Marquee tests now also assert arrows
stay excluded.

## [2.2.1] — 2026-11-06 — The editor keeps its promises

Three block-editor upgrades the interface already hinted at — plus the grip
handle finally earns its drag.

### Added
- **Insert-below palette** (`native/src/blocks/mod.rs`): the grip menu's lone
  «вставить изображение ниже» became a full «Вставить ниже ▸» section —
  hovering expands the whole block palette (paragraph, heading, task list,
  callout, code, data table, audio, image, whiteboard, counter-argument) and
  any kind lands directly below the block whose menu is open, through the
  existing `BlockOp::InsertBelow`.
- **Notion-style «+» in the gutter**: hovering a block shows a small plus
  next to the grip; it opens the same palette and inserts below the hovered
  row. Invisible until hover, so the gutter stays quiet.
- **Slash menu**: typing «/» in a paragraph opens a floating palette under
  the caret, filtered live by what follows the slash — Russian labels match
  in the Russian UI and English ones everywhere (`/callout` finds «Врезка»
  too). Picking a kind converts the paragraph in place; deleting the slash
  or leaving the block closes it. The standing hint «„/“ в пустом абзаце
  открывает блоки» is finally true.
- **Drag-and-drop reorder**: the grip handle now drags. An accent insertion
  line follows the pointer between rows (top half → above, bottom half →
  below) and release lands the move through the new `BlockOp::Reorder`, with
  the index math isolated in `reorder_insert_index` and unit-tested
  (same-gap and one-below moves are no-ops). The cursor turns into a
  grabbing hand while a drag is live.

### Changed
- **Version manifests back in sync**: the v2.2.0 bump moved only
  `native/Cargo.toml`; `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`
  and `package.json` had stayed on 2.1.0. All four now say 2.2.1.
- The grip tooltip now says what the handle actually does — «Меню блока —
  тяните, чтобы переставить» — instead of the generic «Добавить блок».
- Native tests 230 → 233 (reorder math ×2, slash filter).

`docs/releases/v2.2.1.md` tells the story.

## [2.2.0] — 2026-11-06 — Themes, accent, onboarding

The app learns to look like you, and to introduce itself.

### Added
- **Obsidian-style theming** (`native/src/theme.rs` rebuilt): a table of
  **8 complete theme presets** — Graphite (the original dark), Paper, Nord,
  Gruvbox, Catppuccin, Solarized, Everforest and Rosé Pine. Every surface,
  text and status colour is explicit per theme; the derived translucency
  slots (muted/dim/faint/lines/washes) are computed from the theme's ink and
  base, so a new theme is one table row.
- **Personal accent colour** layered over any preset (like Obsidian's
  accent): survives theme switches, and `accent_ink` derives a readable
  companion tint per base automatically.
- **Interface font scale** from 80 % to 140 %, applied to every text style.
- **Appearance picker** (new `native/src/modals/appearance.rs`, open via the
  new «Внешний вид / Appearance» command in ⌘K): theme cards with live
  swatches, accent chips, the scale slider — every change applies instantly.
- **Appearance persistence**: theme id, accent RGB and font scale live in
  `native-appearance.json` next to the locale setting; the legacy
  `native-theme.txt` from 582eb17 is still honoured on first run.
- **Interactive onboarding tour** (new `native/src/modals/onboarding.rs`):
  12 bilingual steps — pages-as-`.eml`, the block editor, the command
  palette, theming, the graph, planning, the Project Knowledge Engine
  (credited to kobaltGIT), offline AI, MCP, import & sync, finish. Steps
  with a "try it" button jump straight into the real feature (palette,
  appearance, graph, planning, integrations, MCP dialog, FAQ); the tour
  hides while the feature is up and resumes from ⌘K with its step kept.
  Auto-opens on a genuinely first launch (persisted in
  `native-onboarding.txt`), `--onboarding` forces it for screenshots, and
  it is gated out of tests and `--snapshot` captures.
- **Demo vault generator** (maintainer tool): `cargo test generate_demo_vault
  -- --ignored` writes a six-page vault with real `todo_list` blocks (ids,
  due dates), tags, parent hierarchy and `[[wiki links]]`.
- **Real-app screenshots regenerated** with the theme system live: page /
  search / planning / graph / onboarding plus `screenshots/themes/` with
  five themes (Nord, Gruvbox, Catppuccin, Paper, Rosé Pine) — all captured
  by the `--snapshot` CLI under Xvfb, no window-manager automation.

### Fixed
- **OCR diagnostics** (mistral.rs v0.9.5 verified against its sources, not
  guessed): the `qwen3_5` vision architecture *is* supported and the CLI
  shape was correct — the real engine panic was only invisible behind
  `Stdio::null()`. Now: a pre-flight check (`preflight()`) validates the
  model folder (weights/tokenizer/config present and non-empty, JSON
  parses) and names the missing file in milliseconds; readiness failures
  embed the **last 12 engine log lines** directly in the UI error; `--isq`
  moved after the `multimodal` subcommand (it belongs to the model-argument
  group at v0.9.5), pinned by a test.

### Tests
- 230 passed, 0 failed, 2 ignored (was 221): preset uniqueness/completeness,
  preset switching, unknown-id safety, accent override persistence,
  font-scale clamping, theme toggle round-trip, onboarding bilingual text +
  done-flag round-trip + last-step semantics, `--onboarding` flag parsing,
  OCR preflight, engine-log tail.

## [2.1.0] — 2026-11-05 — PDF, images, OCR, English voices

Closes every gap named in the 2.0.0 release notes.

### Fixed
- **PDF viewer rendered nothing** (`native/src/pdf.rs`, new): the inline
  preview and the viewer each called `Pdfium::bind_to_library()`, and
  `pdfium-render` 0.9 rejects the second bind
  (`PdfiumLibraryBindingsAlreadyInitialized`), so opening a page after the
  preview always failed. One binding per process now, cached in a thread-local
  and shared by both paths.
- **OCR never started**: `-a qwen3_5` named a loader that this engine build does
  not have, so the process exited instantly. The flag is gone — the engine reads
  `model_type` from the checkpoint's `config.json`; `--no-ui` keeps the bundled
  web chat off the local port.
- **OCR requests were rejected by a healthy engine**: the request sent
  `"model": "ocr"` instead of the id advertised by `/v1/models`
  (`model "ocr" not found`). The id is now read from `/v1/models` at startup.
- **OCR timed out on cold starts**: the readiness window is 15 minutes (was 5)
  and inference gets the same budget.
- **OCR could only read attachments**: the viewer's OCR button now renders the
  current PDF page at 2000 px and recognizes that.
- **STT forced Russian**: whisper was decoded with a hard-coded `<|ru|>`; the
  language now follows the app locale (`whisper::Language`).

### Added
- **Image block** (`native/src/blocks/image.rs`, type `image`): insert a
  picture below any block (*Вставить изображение ниже*) or from the add menu,
  pick a file or reuse an existing attachment, set width 25/50/75/100 % and a
  caption, click to open the viewer. Bytes are stored once in
  `capsule.attachments`; re-picking a file replaces them.
- **English TTS voice** `en_US-amy` (piper medium) in Integrations; the voice
  is picked from the page's script (Cyrillic → Russian, Latin → English) with
  any installed voice as fallback.
- **Custom model folders** (Integrations → *Своя модель*): point STT, TTS or
  OCR at a directory on disk. Paths are validated before use, persisted in
  `ui_state.json` (`custom_models`), and a changed OCR model restarts the
  engine. `components::find_onnx` / `find_with_extension` resolve vendor-named
  voice files, so TTS no longer hard-codes `ru_RU-*.onnx`.

### Changed
- `ai::stt::best_model_dir` and `ai::ocr::resolve_paths` take the custom model
  path and prefer it over the downloaded component.
- `ai::tts::synthesize` finds any `.onnx` in the voice folder.
- `snapshot::encode_png` exposes PNG encoding (used to hand a rendered PDF page
  to the OCR engine); `write_png` builds on it.

## [2.0.0] — 2026-11-03 — Project Knowledge Engine

The 2.0 line: the native egui shell is the product, the vault stays a plain
folder of `.eml` files, and a source tree can now become one of those vaults.

### Added
- **Project Knowledge Engine** (`native/src/project_parser.rs`, ~1.9k lines,
  `docs/PROJECT_KNOWLEDGE_ENGINE.md`) — index a local repository into a
  hierarchical knowledge base inside `.capsule/`: `index.eml` (metrics +
  cross-linked lists), `subsystems/`, `modules/`, `docs/`, plus an aggregated
  technical-debt registry in `tasks_todo.eml` (every `TODO` / `FIXME` /
  `BUG:` / `HACK:` / `NOTE:` with file and line).
  - **Incremental**: a per-file FNV-1a + mtime + size cache in
    `.capsule/.index_state.json`; an unchanged 500-file project re-scans in
    well under 30 ms, and deleted/renamed sources have their stale capsules
    removed instead of left as orphans.
  - **Signature parsing** for Rust (`pub async fn`, `pub struct`, `pub trait`,
    `pub enum`, multi-line generics), TypeScript/JavaScript (`export async
    function`, `export interface`, `export type`, `export class`), Python,
    Go, C#, Java, Dart, plus Markdown documentation, with `///`, `//!`,
    JSDoc/Javadoc and Python docstring extraction.
  - **Zero data loss on re-sync** (`write_capsule_preserving`): manually added
    paragraphs, checklists, callouts, user tags, capsule status and the
    whiteboard `canvas_state.json` are extracted before a managed capsule is
    rewritten; the legacy flat layout (`module_foo.eml`) migrates into
    `modules/` automatically.
- **Agent skills scaffolding** (`native/src/project_skill.rs`) — the scan
  generates skill files and MCP wiring so an autonomous agent opens the
  project with real context (public API, subsystems, tech debt) instead of
  guessing; verification helpers live in `project_verify.rs`.
- **Project open + re-sync from the UI** — "open project as knowledge base"
  in the palette (native folder dialog via `rfd`) and a sidebar re-sync
  button that reports updated/removed file counts.
- **Folder folding in the sidebar** — collapse/expand every subfolder in one
  click; `index.eml` stays open and nested folders start collapsed, so a
  repository with hundreds of modules does not bury the overview in noise.

### Changed
- Version lines unified at **2.0.0** across `package.json`, `native/Cargo.toml`
  and `src-tauri` (they had drifted to 1.12.2 / 1.12.4 / 1.12.0).

## [1.12.4] — 2026-11-02 — OCR you can actually see working

### Fixed
- **The engine no longer hangs on "инициализация движка" forever**
  (`native/src/ai/ocr.rs`): mistral.rs was spawned with piped
  stdout/stderr that nobody ever read — once the engine filled the 64 KB
  pipe buffer with its startup log it blocked mid-load, and the app waited
  out the full five-minute timeout with zero visible progress. The pipes
  are now `null()`, and a process that dies before readiness fails
  immediately with its exit status instead of burning the timeout.
- **The OCR result is no longer swallowed on imported email pages**
  (`native/src/main.rs`): a recognized text was appended as a paragraph
  *block*, but `.eml` pages render read-only and never draw blocks — the
  toast promised "added to the page" while the text went nowhere. Now a
  read-only page copies the recognized text to the clipboard and says
  exactly that; native pages still get it appended as a block.
- **STT toasts said "OCR"** — transcription finished with «Текст распознан
  и добавлен на страницу»; it now has its own «Расшифровка готова» copy.

### Added
- **A live OCR status card** (`native/src/main.rs`): bottom-right card with
  a spinner, the file name, the phase («Запускаю движок, загружаю модель
  (~2 ГБ)…» → «Движок готов, распознаю текст…») and a moving elapsed timer.
  A cold start of the model takes minutes; a toast that vanished in two
  seconds was simply not the truth. The card also explains that the first
  run is the slowest.
- **A persistent result card**: when recognition finishes, the recognized
  text stays on screen (scrollable, selectable) with a «Скопировать»
  button and an explicit line about where it went — «Текст добавлен в конец
  страницы — блок подсвечен» or «Страница-письмо только для чтения: текст
  скопирован в буфер обмена».
- **The fresh paragraph announces itself**: on native pages the document
  scrolls to the new block and strokes it with the accent color for a few
  seconds, so "куда оно добавилось?" is answered visually, not by faith.
- **The viewer's OCR button becomes a spinner** while the engine runs
  (`native/src/viewer.rs`); clicking OCR again elsewhere answers «OCR уже
  запущен — подожди завершения (scan.png)» instead of resetting silently.

## [1.12.3] — 2026-11-02 — Documents live on the page; AI entry points everywhere

### Fixed
- **The PDF viewer is a real viewer now** (`native/src/viewer.rs`): pages
  scroll in both axes instead of clipping at the window edge (a page taller
  than the screen was simply cut off), flipping a page or zooming starts at
  the top of the new page, and the overlay no longer closes on any click —
  a stray click used to throw away the open document. Pages flip with ← →,
  zoom works with +/− or the toolbar, «По ширине» refits the page, and the
  zoom-out button finally has a minus icon instead of a download arrow.
- **Attached PDFs and images are visible on the page itself**
  (`native/src/preview.rs`, `views/document_view.rs`): an image attachment
  renders as a picture under the text and a PDF as its first page with a
  "PDF · N страниц" caption — one click opens the full-screen viewer.
  Imported `.eml` pages get the same treatment plus a read-only attachment
  list with view / OCR / save; previously an imported email hid its
  attachments behind the "Preview" button entirely.
- **TTS/OCR no longer dead-end in a toast**: «Озвучить страницу» with the
  sherpa-onnx runtime or a voice missing (and OCR / STT / PDF-viewing with
  their engines missing) now opens the Integrations modal directly, where
  the required component can be installed in two clicks. The e2e suite
  covers OCR, TTS and PDF paths.
- **OCR is reachable from more places**: the image viewer toolbar gained an
  OCR button (no need to close the viewer and find the attachment row), and
  the command palette gained «Озвучить страницу» (⌘K → TTS), so both local
  AI features are discoverable without knowing where the rows hide.

## [1.12.2] — 2026-11-02 — The Integrations button actually opens; installers for every platform

### Fixed
- **The ⚙ Integrations button did nothing at all.** The v1.12.0 branch added
  the `open_integrations` flag and the title-bar chip, but `ShellAction::absorb`
  — the step that merges title-bar actions into the shell — never copied the
  new field, so the click was silently dropped and the modal never opened.
  Covered by a new headless end-to-end test that drives the REAL app frame
  with synthetic mouse clicks and sweeps the title bar until the modal opens.
- Integrations is now also reachable from the command palette (⌘K →
  "Интеграции / Integrations"), so the catalog has a second entry point.

### Changed
- **Installers instead of bare archives.** Windows ships an NSIS
  `…_x64-setup.exe` (start-menu/desktop shortcuts, uninstaller) alongside the
  portable zip; macOS ships a drag-to-Applications `.dmg` (universal, Apple
  Silicon + Intel); Linux ships a proper `.deb` next to the zero-dependency
  AppImage.

## [1.12.1] — 2026-11-02 — Native-only releases; Integrations fixes

### Fixed
- **Integrations modal swallowed clicks** (`native/src/modals/integrations.rs`):
  the dialog panel was only painted while a full-screen click-sensing backdrop
  sat underneath — any poke that missed a button (status pills, progress bars,
  empty card space) fell through to the backdrop and closed the whole modal.
  The panel now claims its own click area; dismissal works as before, on
  clicks outside the dialog or Escape.
- **TTS runtime downloaded an incompatible sherpa-onnx**: the catalog fetched
  v2.0.1 while the hand-verified FFI in `ai/tts.rs` mirrors `c-api.h` at
  v1.12.40 (v2.0.1 dropped the `data_dir` phonemization argument). The
  component now downloads v1.12.40, and a regression test welds the URL to
  the FFI tag.
- **OCR engine URLs were welded to a release tag**: `mistralrs-engine-*.tar.gz`
  is now fetched through the `releases/latest` redirect, so component
  downloads survive tag bumps without a code change.

### Changed
- **Releases ship native builds only.** The Tauri/web job (MSI, NSIS, deb,
  rpm, dmg) is gone from the release pipeline — the native egui shell is the
  product. Artifacts per release: Windows x64 portable zip, Linux amd64
  zero-dependency AppImage, macOS universal `.app`, plus the mistral.rs OCR
  engines and a unified `SHA256SUMS.txt`.
- macOS builds are now **universal binaries** (lipo of aarch64 + x86_64):
  one `.app` for Apple Silicon and Intel Macs.

## [1.12.0] — 2026-11-01 — Integrations: local OCR/STT/TTS you download in the app, PDF and image viewing

The native shell gains a components architecture: a catalog of downloadable,
offline engines behind a new ⚙ **Integrations** modal. Nothing is bundled —
the vault stays zero-dependency and a component only appears on disk after an
explicit download, outside the vault folder. Native tests grow from 151 to
**178**.

### Added
- **Components core (`native/src/components.rs`)** — a static catalog of nine
  components (OCR engine, OCR model, three whisper STT models, TTS runtime,
  two Russian voices, PDF renderer) with per-OS URLs, dependency gating
  (a voice requires the runtime, the OCR model requires the engine), a
  threaded download manager with streaming progress, tar.gz/tar.bz2 unpacking
  hardened against path escapes and symlink handling, and `components.json`
  install markers in the app-data directory.
- **OCR: Xiaomi-OCR-0 on mistral.rs** — the new Xiaomi OCR model (Qwen3.5-0.8B
  hybrid-attention VLM, Apache-2.0, 1.75 GB bf16) served by the pure-Rust
  mistral.rs engine v0.9.5 (MIT; no llama.cpp, no Python). The engine runs as
  a localhost actor process (`mistralrs serve multimodal -a qwen3_5`, startup
  validated in CI via `serve multimodal --help`); the app posts images to its
  OpenAI-compatible endpoint and appends the recognized text as a paragraph.
  Entry point: ✨ button on any image attachment row.
- **STT: whisper in-process on candle** — openai/whisper tiny/base/small run
  inside the app (pure Rust, CPU) with the OpenAI timestamp-rules decoder
  ported from candle-examples; audio is decoded by a built-in RIFF/WAVE parser
  (8/16/24/32-bit PCM and float, any channel count) and linearly resampled to
  16 kHz. Russian is forced via the `<|ru|>` token. Entry point: «Расшифровать
  запись» in the audio block; the transcript lands back in the block.
- **TTS: Russian voices via sherpa-onnx** — piper VITS voices (Ирина, Дмитрий)
  through the sherpa-onnx v1.12.40 shared library loaded at runtime with
  hand-verified FFI (struct layouts mirrored from c-api.h at that exact tag;
  v2.0.1 was rejected because it dropped `data_dir` phonemization). The page
  is spoken from the document editor (🔊 button); audio plays through the
  existing rodio player. Verified end-to-end against the real library and
  voice: ~3 s of Russian audio synthesized in 1.2 s on CPU (kept as an
  env-gated smoke test).
- **PDF viewer** — pdfium (chromium/8125, matching pdfium-render 0.9's
  bindgen) loaded from the components directory; page navigation, zoom,
  continuous re-render on resize, white page surface in the dark UI.
- **Image viewer** — PNG/JPEG/GIF/WebP decode via the `image` crate, zoom
  20–500 %, pan via scroll areas.
- **Integrations modal (⚙ in the title bar)** — grouped catalog cards with
  status pills (Ready / Installed / Missing components / Failed), live
  download progress bars, per-component size and version, open-folder and
  delete actions, and download-driven repaint pacing.
- **CI: engine artifacts** — a release-workflow job builds the mistral.rs CLI
  for linux-x64, macos-arm64 and windows-x64, validates the serve subcommand
  shape, and attaches `mistralrs-engine-<platform>.tar.gz` to the release; the
  finalize guard treats a failed engine build as an incomplete release.

### Notes
- pdfium and sherpa-onnx are the only non-Rust pieces in the whole feature,
  and they are renderer/codec runtimes loaded at runtime, not inference
  engines — the OCR model itself runs on a Rust engine by design.
- Downloads have no resume; the largest component is the 1.75 GB OCR model.

## [1.11.0] — 2026-11-01 — Polish release: blockers, undo, scrubbing

Three long-standing limitations resolved across both surfaces. Native tests
grow from 143 to **151**; web tests from 175 to **176**.

### Added
- **Gantt: multiple blockers per task (web)** — the dates-and-dependencies
  editor replaces the single-prerequisite `<select>` with a checkbox panel
  over the whole `blockedBy: string[]` array; cycle-creating candidates are
  shown but disabled, unchecking an existing edge stays allowed. The native
  context menu already toggled additively (`src/components/PlanningView.tsx`).
- **Whiteboard: undo and multi-select (native)** — an undo stack (up to 40
  steps: add, move, delete, reset) behind a new `Icon::Undo`; marquee
  selection on empty canvas space; dragging any selected element moves the
  whole group; Delete clears every selected element
  (`native/src/blocks/canvas.rs`).
- **Audio: seek and volume (native)** — a scrubbable progress bar with
  `m:ss / total` labels and a mini volume slider next to the transport
  buttons. The player keeps the decoder un-buffered so rodio's
  `Sink::try_seek` works, mirrors the playhead into shared state on its poll,
  and applies the session volume to this and future tracks
  (`native/src/audio_player.rs`).

### Fixed
- The whiteboard hint claimed "drag to pan, wheel to zoom" — the canvas never
  panned or zoomed; the hint now describes what actually happens.

## [1.10.0] — 2026-11-01 — Microphone recording in the native shell

The record half of the audio item: the egui shell now captures the microphone
straight from the audio block, closing the gap with the web `MediaRecorder`
flow. Native test suite grows from 135 to **143** unit tests.

### Added
- **Mic recording from the audio block**: a "Record from mic" button next to
  "Attach audio" starts a take with a pulsing dot, an honest `m:ss` clock and
  a live RMS level meter; "Stop recording" lands the take as a 16-bit mono
  PCM WAV — cpal capture at the device's native rate, any input layout
  downmixed — attached to the capsule like any other MIME part and immediately
  playable, because the offline player decodes WAV natively
  (`native/src/audio_recorder.rs`).
- Takes are capped at ten minutes so a forgotten stop cannot fill the disk;
  the level meter reads real input, never a fake animation.

### Fixed
- **File attachments from the audio block actually land in the capsule now**:
  the file picker wrote a `__attach` marker into the block JSON that nothing
  ever read. Attachments now travel through a new `BlockOp::AttachAudio`
  command that the document view applies to `capsule.attachments` — the same
  path recordings use.
- Release pipeline: the native AppImage job installs `libasound2-dev` +
  `pkg-config`, fixing the v1.9.0 AppImage failure (`alsa-sys` had no ALSA
  headers on ubuntu-22.04).

### Changed
- New `Icon::Record` (filled dot) in the native icon set; the whole native
  crate is `cargo fmt`-normalized.

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
