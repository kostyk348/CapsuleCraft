# Contributing to CapsuleCraft

[English](#english) · [Русский](#русский)

---

## English

Thanks for taking the time to help. CapsuleCraft is small on purpose — please keep changes focused.

### Ground rules

1. **The `.eml` format is the contract.** A capsule must stay valid RFC 5322 + MIME. Never introduce a side channel that only CapsuleCraft can read.
2. **No new runtime infrastructure.** No database, no server, no telemetry, no external API calls at runtime.
3. **Local-first.** Offline behaviour is the default path, not a fallback.
4. **Zero-DB, not zero-structure.** If a feature needs state, it goes into the capsule or the vault folder.

### Getting started

```bash
git clone https://github.com/kostyk348/CapsuleCraft.git
cd CapsuleCraft/native
cargo run --release -- /path/to/your/vault
```

Requirements: Rust stable. There is no browser engine, no Node.js and no WebView in the build — the app is a standalone egui shell (`native/`).

### Before you open a PR

```bash
cargo fmt --manifest-path native/Cargo.toml --check
cargo clippy --manifest-path native/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path native/Cargo.toml -j 1
```

CI runs exactly these checks on Linux; tags also build the release artifacts (Windows, AppImage, macOS, Android APK).

### Style

- Rust: `cargo fmt` defaults, no `unwrap()` on user-controlled paths.
- Commits: [Conventional Commits](https://www.conventionalcommits.org/) — `feat:`, `fix:`, `docs:`, `chore:`, `ci:`.
- UI copy: English and Russian strings should stay consistent if you touch labels (`native/src/i18n.rs`).

### Adding a block type

1. Add the block data model in `native/src/blocks/` (serialization is plain MIME/RFC 5322 headers + parts).
2. Implement the renderer next to the other block renderers.
3. Register it in the document view and the command palette.
4. Check that an exported `.eml` round-trips back into the same capsule.

### Reporting bugs

Use the issue templates. Useful details: OS, CapsuleCraft version, what you expected, what happened, and — if possible — the offending `.eml` with private content removed.

Security issues: please do **not** open a public issue, mail the maintainer instead.

---

## Русский

Спасибо, что помогаете. Проект намеренно небольшой — держите изменения сфокусированными.

### Основные правила

1. **Формат `.eml` — это контракт.** Капсула должна оставаться валидным RFC 5322 + MIME. Никаких побочных каналов, читаемых только CapsuleCraft.
2. **Никакой новой инфраструктуры.** Ни БД, ни сервера, ни телеметрии, ни внешних API в рантайме.
3. **Local-first.** Офлайновое поведение — основной путь, а не запасной.
4. **Zero-DB, но не zero-structure.** Нужно состояние — оно едет в капсуле или в папке хранилища.

### Начало работы

```bash
git clone https://github.com/kostyk348/CapsuleCraft.git
cd CapsuleCraft/native
cargo run --release -- /path/to/your/vault
```

Требования: Rust stable. В сборке нет браузерного движка, Node.js и WebView — приложение это standalone-шелл на egui (`native/`).

### Перед PR

```bash
cargo fmt --manifest-path native/Cargo.toml --check
cargo clippy --manifest-path native/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path native/Cargo.toml -j 1
```

CI гоняет ровно эти проверки на Linux; на тегах собираются релизные артефакты (Windows, AppImage, macOS, Android APK).

### Стиль

- Rust: дефолты `cargo fmt`, без `unwrap()` на путях, приходящих от пользователя.
- Коммиты: [Conventional Commits](https://www.conventionalcommits.org/) — `feat:`, `fix:`, `docs:`, `chore:`, `ci:`.
- Тексты интерфейса: если трогаете подписи, держите английский и русский варианты согласованными (`native/src/i18n.rs`).

### Новый тип блока

1. Добавьте модель данных блока в `native/src/blocks/` (сериализация — обычные заголовки и части MIME/RFC 5322).
2. Реализуйте рендер рядом с остальными рендерерами блоков.
3. Зарегистрируйте его в документе и в палитре команд.
4. Проверьте, что экспортированный `.eml` читается обратно в ту же капсулу.

### Баг-репорты

Используйте шаблоны issue. Полезно указать: ОС, версию CapsuleCraft, ожидаемое и фактическое поведение, а по возможности — проблемный `.eml` без приватных данных.

Об уязвимостях: **не** открывайте публичный issue, напишите мейнтейнеру.
