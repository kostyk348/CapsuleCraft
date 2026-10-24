# Архитектура и руководство мейнтейнера: Obsidian & Markdown Importer

Документ описывает внутреннее устройство, алгоритмы и сценарии работы встроенного импортера баз знаний **Obsidian** / **Markdown** в нативном клиенте **CapsuleCraft** (`capsuleos-native`).

---

## 1. Концепция и цели

CapsuleCraft хранит страницы в формате **Zero-DB Pure-MIME RFC 5322 (`.eml`)**, где каждая страница — это автономный почтовый файл, содержащий метаданные в заголовках, JSON-состояние блоков в `canvas_state.json` и вложения в multipart-частях.

Цель модуля импорта (`native/src/importers/obsidian.rs`) — обеспечить бесшовную миграцию любых существующих баз знаний Obsidian в CapsuleCraft без потери:
- Иерархии вложенных папок;
- Метаданных YAML frontmatter (`title`, `tags`, `aliases`, `date`, `parent`);
- Ссылок `[[wikilinks]]` и `[[target|alias]]`;
- Встроенных чекбоксов и списков задач (`- [ ]`, `- [x]`);
- Блоков цитат и Obsidian Callouts (`> [!NOTE]`, `> [!TIP]`, `> [!WARNING]`);
- Встроенных изображений и ассетов (`![[image.png]]`, `![alt](photo.jpg)`).

> [!IMPORTANT]
> **Принцип неизменяемости источника (Read-Only Source):** Исходное хранилище Obsidian никогда не модифицируется импортером. Все данные читаются и преобразуются в новые `.eml` файлы в целевой папке хранилища CapsuleCraft.

---

## 2. Архитектурная схема пайплайна

```
┌─────────────────────────────────────────────────────────────┐
│                 Obsidian Vault Directory                    │
│   ├── Projects/                                             │
│   │   ├── Architecture.md                                   │
│   │   └── Subfolder/Note.md                                 │
│   ├── assets/diagram.png                                    │
│   └── Daily/2026-10-24.md                                   │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
 1. Indexing & Asset Discovery 
    ├── Составление карты ассетов: filename.to_lowercase() -> PathBuf
    └── Сбор относительных путей всех .md файлов
                               │
                               ▼
 2. Folder Hierarchy Resolution 
    ├── Построение дерева директорий
    ├── Поиск естественных индексных заметок (index.md, README.md, Name.md)
    └── Генерация виртуальных папок-капсул с Message-ID <obsidian-folder-...>
                               │
                               ▼
 3. Parsing & Conversion (per note) 
    ├── parse_frontmatter(): YAML заголовки (tags, aliases, date, parent)
    ├── parse_markdown_to_blocks(): разбивка на блоки canvas_state.json
    └── extract_attachment_references(): сбор картинок ![[...]] и ![...](...)
                               │
                               ▼
 4. Multipart Assembly & Emission 
    ├── Поиск бинарных файлов в карте ассетов, кодирование в Base64
    ├── Заполнение RFC 5322 заголовков (X-Capsule-Page-Parent, X-Capsule-Tags)
    ├── Сборка через assemble_eml(&capsule)
    └── Атомарная запись <slug>.eml в CapsuleCraft Vault
```

---

## 3. Внутреннее устройство модулей

### 3.1. `parse_frontmatter(content: &str) -> (Frontmatter, &str)`
- Корректно удаляет UTF-8 BOM (`\u{feff}`) при наличии.
- Ищет начальный разделитель `---` и замыкающий `---` / `...`.
- Поддерживает:
  - Строковые поля: `title: "..."`, `date: ...`, `parent: ...`;
  - Списки тегов и псевдонимов в стилях:
    - YAML-список с дефисами:
      ```yaml
      tags:
        - tag1
        - tag2
      ```
    - Inline-массив: `tags: [tag1, tag2]`;
    - Через запятую: `tags: tag1, tag2`.
  - Автоматически очищает символы `#` в тегах (`#rust` -> `rust`).

### 3.2. `parse_markdown_to_blocks(md: &str) -> Vec<Value>`
Преобразует текст заметки в массив JSON-блоков:
| Исходный Markdown | Тип блока CapsuleCraft | Поля данных |
|---|---|---|
| `# H1`, `## H2`, `### H3` | `"heading"` | `level: 1..3`, `text: "..."` |
| `- [ ] Task`, `- [x] Done` | `"todo_list"` | `items: [{ id, text, done, status, priority }]` |
| `> [!NOTE] Title`<br>`> Body` | `"callout"` | `title: "...", content: "...", icon: "info", tone: "blue"` |
| `> [!TIP] Title` | `"callout"` | `icon: "lightbulb", tone: "green"` |
| `> [!WARNING] Title` | `"callout"` | `icon: "alert", tone: "amber"` |
| `> Quote text` | `"callout"` | `icon: "quote", tone: "gray", title: ""` |
| ` ```rust ... ``` ` | `"code_repl"` | `language: "rust", code: "..."` |
| Обычный текст | `"paragraph"` | `text: "..."` |

### 3.3. Разрешение иерархии папок и `X-Capsule-Page-Parent`
1. Каждая директория внутри Obsidian мапится в уникальный идентификатор:
   `message_id = <obsidian-folder-{slug}@vault.local>`.
2. Если в папке нет одноименной заметки (например, для папки `Projects/` нет `Projects.md`, `index.md` или `README.md`), создается виртуальная страница-контейнер с заголовком папки.
3. Заметки внутри этой папки автоматически получают заголовок:
   ```email
   X-Capsule-Page-Parent: <obsidian-folder-projects@vault.local>
   ```
4. Если во frontmatter заметки явно указан `parent: Target` или `up: Target`, приоритет отдается ему.

### 3.4. Обработка вложений и изображений
- Функция `extract_attachment_references()` сканирует как синтаксис Obsidian `![[image.png]]`, так и стандартный Markdown `![alt](path/image.png)`.
- Поиск ассетов производится по глобальной карте файлов исходного хранилища (независимо от того, лежат ли картинки рядом, в `assets/` или `attachments/`).
- Найденный файл пакуется в структуру `Attachment`:
  - `name`: имя файла (например, `diagram.png`);
  - `cid`: `<diagram-png@vault.local>`;
  - `mime_type`: определяется автоматически через `guess_mime()`;
  - `raw_b64`: Base64 payload.
- Сборщик `assemble_eml()` упаковывает их в `multipart/mixed` часть письма.

---

## 4. Сценарии использования

### 4.1. Запуск через интерфейс командной строки (CLI)

```powershell
# Импорт с явным указанием целевого хранилища:
cargo run --package capsuleos-native -j 1 -- import-obsidian --from "C:/Users/Name/Documents/MyObsidianVault" --vault "D:/Vaults/CapsuleVault"

# Либо через флаг:
cargo run --package capsuleos-native -j 1 -- --import-obsidian="C:/Users/Name/Documents/MyObsidianVault"
```

Вывод команды предоставляет полную статистику:
```text
Successfully imported Obsidian vault into D:\Vaults\CapsuleVault
- Pages imported: 42
- Folders created: 6
- Attachments imported: 15
```

### 4.2. Запуск через графический интерфейс (GUI)
1. Нажмите `Ctrl + K` для вызова командной палитры;
2. Начните вводить `obsidian` или `импорт`;
3. Выберите пункт **«Импорт Obsidian / Markdown…»**;
4. В открывшемся системном диалоге выберите корневую папку Obsidian-хранилища;
5. Хранилище CapsuleCraft моментально перечитает новые `.eml` файлы, обновит боковое дерево страниц и покажет toast-уведомление с количеством добавленных страниц и вложений.

---

## 5. Тестирование и верификация

Набор тестов расположен в `native/src/importers/obsidian.rs` и `native/src/cli.rs`.
Запуск тестов:

```powershell
cargo test --manifest-path native/Cargo.toml obsidian -j 1
```

Покрыты сценарии:
- `test_parse_frontmatter`: разбор сложных YAML frontmatter с тегами, псевдонимами и датами;
- `test_markdown_to_blocks`: трансформация всех типов Markdown в JSON блоки;
- `test_extract_attachments`: извлечение имен файлов из wikilinks и markdown-ссылок;
- `test_import_vault_flow`: сквозной цикл импорта временной структуры директорий с генерацией валидных `.eml` файлов;
- `cli::tests::parses_import_obsidian_flags`: парсинг CLI аргументов `--from` и `--vault`.
