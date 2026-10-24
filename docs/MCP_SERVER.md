# Сервер Model Context Protocol (MCP) — Руководство для мейнтейнера

Данный документ содержит полное техническое описание архитектуры, протокола, ресурсов и всех инструментов встроенного MCP-сервера CapsuleCraft / CapsuleOS (`native/src/mcp.rs`).

---

## 1. Назначение и архитектура

Встроенный MCP-сервер превращает локальное файловое хранилище CapsuleCraft в активный **Second Brain** для автономных ИИ-агентов (Google Antigravity, Claude Code, Cursor, Windsurf, Claude Desktop).

### Ключевые характеристики
* **Протокол:** JSON-RPC 2.0 (построчный line-delimited over stdio).
* **Спецификация MCP:** `2024-11-05`.
* **Имя сервера (`serverInfo.name`):** `capsulecraft-mcp`.
* **Текущая версия (`serverInfo.version`):** `1.5.0`.
* **Транспорт:** стандартные потоки ввода-вывода (stdin/stdout). Диагностические логи пишутся строго в `stderr`, не нарушая парсинг JSON-RPC.

### Ключевые архитектурные инварианты
1. **Zero-DB & Pure-MIME (RFC 5322):**
   * Все страницы хранятся как стандартные `.eml` файлы. База данных отсутствует.
   * Структурированные блоки страницы сериализуются в MIME-часть `canvas_state.json` внутри `multipart/alternative`, дублируясь доступным HTML и plain-text представлением.
2. **Безопасная атомарность (Safe Concurrency):**
   * Перед выполнением любого инструмента (`call_tool`) и чтением ресурсов сервер вызывает `self.reload()`, подтягивая с диска свежие изменения, сделанные пользователем в GUI или внешними процессами.
   * При записи (`capsule_create`, `capsule_reply`, `capsule_patch`) файл генерируется заново через `mime::assemble_eml`, гарантируя сохранение всех заголовков, связей веток и вложений.
3. **Разграничение нативных и импортированных писем:**
   * Капсулы, созданные CapsuleCraft (`native: true`), доступны для чтения и модификации.
   * Внешние импортированные email-сообщения без блока `canvas_state.json` помечаются как `native: false` и защищены от случайной перезаписи: операции модификации (`capsule_patch`) возвращают информативную ошибку.
4. **Двуязычный поисковый движок:**
   * Токенизатор `search::tokenize` одинаково корректно обрабатывает латиницу и кириллицу, обеспечивая работу Okapi BM25 (`capsule_search`) и TF-IDF косинусного сходства (`capsule_related`) на русском и английском языках без тяжелых внешних зависимостей.

---

## 2. Запуск и конфигурация клиентов

### Запуск из командной строки
```powershell
# Запуск MCP сервера на активном хранилище (по умолчанию ~/Capsules или последний открытый)
capsuleos-native --mcp

# Запуск с явным указанием пути к хранилищу
capsuleos-native --mcp "D:\MyVault"

# Вывод готового JSON-конфига для различных клиентов
capsuleos-native --mcp-config
```

### Конфигурация для Antigravity / Cursor / Claude Desktop
В файл конфигурации MCP клиента (`mcpSettings.json` или `claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "capsulecraft": {
      "command": "C:\\Path\\To\\capsuleos-native.exe",
      "args": ["--mcp"]
    }
  }
}
```

### Конфигурация для Claude Code CLI
```powershell
claude mcp add capsulecraft "C:\Path\To\capsuleos-native.exe" --mcp
```

---

## 3. Ресурсы (MCP Resources)

Сервер предоставляет доступ к страницам хранилища в виде стандартных MCP Resources.

### 3.1. `resources/list`
Возвращает список всех страниц хранилища.
* **Схема URI:** `capsule://<file_name>` (например, `capsule://capsule_native_a1b2c.eml`).
* **MIME-тип:** `text/plain`.

### 3.2. `resources/read`
Возвращает текстовое содержимое капсулы (заголовок + контент всех текстовых блоков).
* **Параметры:** `uri: string` (принимает `capsule://<file_name>`, `file_name` или RFC `Message-ID`).

---

## 4. Справочник инструментов (Tools Reference)

Сервер предоставляет 9 специализированных инструментов:

| Инструмент | Назначение |
| :--- | :--- |
| [`capsule_list`](#41-capsule_list) | Получение списка всех капсул с метаданными и тегами |
| [`capsule_read`](#42-capsule_read) | Чтение полного содержимого страницы, блоков и вложений |
| [`capsule_search`](#43-capsule_search) | Полнотекстовый ранжированный поиск Okapi BM25 |
| [`capsule_create`](#44-capsule_create) | Создание новой страницы со структурированными блоками |
| [`capsule_reply`](#45-capsule_reply) | Добавление ответа в RFC 5322 тред с ветвлением контекста |
| [`capsule_patch`](#46-capsule_patch) | Атомарный патч (добавление текста, чекбоксы, теги, замена) |
| [`capsule_backlinks`](#47-capsule_backlinks) | Получение графа обратных связей заметки (`[[Wiki links]]`) |
| [`capsule_related`](#48-capsule_related) | Семантический подбор похожих страниц по TF-IDF сходству |
| [`capsule_tasks`](#49-capsule_tasks) | Поиск и фильтрация всех задач хранилища по дедлайнам и статусам |

---

### 4.1. `capsule_list`

Получение каталога капсул хранилища с метаданными.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "tag": {
      "type": "string",
      "description": "Опциональный фильтр по тегу (без учета регистра, например 'architecture', 'decision')"
    }
  }
}
```

#### Пример запроса
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "capsule_list",
    "arguments": {
      "tag": "architecture"
    }
  }
}
```

#### Пример ответа
```json
{
  "count": 1,
  "capsules": [
    {
      "id": "<capsule-native-9e246-1792844358214@vault.local>",
      "subject": "Distributed Consensus Architecture",
      "from": "Author <author@vault.local>",
      "to": "CapsuleCraft Vault <vault@capsuleos.local>",
      "date": "Sat, 24 Oct 2026 15:00:00 +0300",
      "tags": ["architecture", "scaling"],
      "status": "active",
      "parent_id": null,
      "file_name": "capsule_native_9e246.eml",
      "native": true,
      "blocks_count": 4,
      "attachments_count": 0
    }
  ]
}
```

---

### 4.2. `capsule_read`

Чтение полного структурированного содержимого страницы.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Message-ID (e.g. <alpha@vault.local>), имя файла (.eml) или заголовок страницы"
    }
  },
  "required": ["id"]
}
```

#### Пример ответа
```json
{
  "id": "<capsule-native-9e246-1792844358214@vault.local>",
  "subject": "Distributed Consensus Architecture",
  "from": "Author <author@vault.local>",
  "to": "CapsuleCraft Vault <vault@capsuleos.local>",
  "date": "Sat, 24 Oct 2026 15:00:00 +0300",
  "tags": ["architecture"],
  "status": "active",
  "parent_id": null,
  "in_reply_to": null,
  "references": [],
  "file_name": "capsule_native_9e246.eml",
  "content": "Distributed Consensus Architecture\nОсновной текст заметки с подробным описанием.",
  "blocks": [
    {
      "id": "blk_native_paragraph_12345",
      "type": "paragraph",
      "data": { "text": "Основной текст заметки с подробным описанием." }
    }
  ],
  "attachments": []
}
```

---

### 4.3. `capsule_search`

Ранжированный полнотекстовый поиск Okapi BM25 с генерацией сниппетов.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Поисковый запрос (поддерживает русский и английский стемминг/слова)"
    },
    "limit": {
      "type": "integer",
      "description": "Максимальное количество результатов (по умолчанию: 10)"
    }
  },
  "required": ["query"]
}
```

#### Пример ответа
```json
{
  "query": "Consensus",
  "total_matches": 1,
  "results": [
    {
      "id": "<capsule-native-9e246-1792844358214@vault.local>",
      "file_name": "capsule_native_9e246.eml",
      "subject": "Distributed Consensus Architecture",
      "score": 2.415,
      "snippet": "…детальное описание протокола Consensus и алгоритма Raft…",
      "tags": ["architecture"],
      "date": "Sat, 24 Oct 2026 15:00:00 +0300"
    }
  ]
}
```

---

### 4.4. `capsule_create`

Создание новой капсулы в хранилище со структурированными блоками.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "subject": {
      "type": "string",
      "description": "Тема / заголовок создаваемой капсулы"
    },
    "content": {
      "type": "string",
      "description": "Основной текст (разбивается пустыми строками \\n\\n на абзацы)"
    },
    "author": {
      "type": "string",
      "description": "Имя или идентификатор автора (по умолчанию: 'AI Agent')"
    },
    "tags": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Список тегов (например ['ai', 'research'])"
    },
    "parent_id": {
      "type": "string",
      "description": "Опциональный Message-ID родительской капсулы для помещения в иерархию"
    },
    "callouts": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "text": { "type": "string" },
          "tone": {
            "type": "string",
            "enum": ["emerald", "amber", "rose", "indigo"],
            "description": "Цветовой акцент: emerald (успех), amber (предупреждение), rose (риск), indigo (инфо)"
          }
        },
        "required": ["text"]
      },
      "description": "Опциональные акцентные плашки (callout boxes)"
    },
    "todos": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Опциональные пункты чек-листа / задач"
    }
  },
  "required": ["subject"]
}
```

#### Пример ответа
```json
{
  "success": true,
  "id": "<capsule-native-7f31a-1792844500112@vault.local>",
  "file_name": "capsule_native_7f31a.eml",
  "subject": "План тестирования распределенного хранилища",
  "status": "active",
  "blocks_count": 3,
  "parent_id": null
}
```

---

### 4.5. `capsule_reply`

Создание ответа в треде с формированием почтовых заголовков RFC 5322 (`In-Reply-To`, `References`).

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "parent_id": {
      "type": "string",
      "description": "Message-ID или имя файла родительской капсулы, на которую дается ответ"
    },
    "content": {
      "type": "string",
      "description": "Текст ответа"
    },
    "subject": {
      "type": "string",
      "description": "Тема ответа (по умолчанию: 'Re: <тема родителя>')"
    },
    "author": {
      "type": "string",
      "description": "Автор реплики (по умолчанию: 'AI Opponent')"
    }
  },
  "required": ["parent_id", "content"]
}
```

#### Пример ответа
```json
{
  "success": true,
  "id": "<capsule-native-8c41f-1792844510340@vault.local>",
  "file_name": "capsule_native_8c41f.eml",
  "subject": "Re: План тестирования распределенного хранилища",
  "parent_id": "<capsule-native-7f31a-1792844500112@vault.local>",
  "in_reply_to": "<capsule-native-7f31a-1792844500112@vault.local>",
  "blocks_count": 1
}
```

---

### 4.6. `capsule_patch`

Атомарная модификация существующей нативной капсулы без перезаписи чужих блоков.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Message-ID, заголовок или имя файла капсулы для модификации"
    },
    "append_markdown": {
      "type": "string",
      "description": "Markdown текст для добавления новых абзацев в конец заметки"
    },
    "toggle_task": {
      "type": "string",
      "description": "Текст задачи или ее ID для переключения между [ ] и [x]"
    },
    "add_tags": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Список новых тегов для добавления к странице"
    },
    "action": {
      "type": "string",
      "enum": ["append_text", "replace_text", "toggle_task", "add_tag"],
      "description": "Явное указание действия (альтернативный синтаксис)"
    },
    "target": {
      "type": "string",
      "description": "Целевая подстрока для replace_text, либо задача/тег для toggle/add"
    },
    "content": {
      "type": "string",
      "description": "Текст замены для replace_text или текст для добавления"
    }
  },
  "required": ["id"]
}
```

#### Поддерживаемые операции
1. **Добавление абзацев (`append_markdown` или `action: "append_text"`):** разбивает текст по `\n\n` и добавляет блоки типа `paragraph`.
2. **Переключение задач (`toggle_task` или `action: "toggle_task"`):** ищет в блоках `todo_list` задачу по ID или тексту (с поиском подстроки без учета регистра), инвертирует статус `done` (`false` $\leftrightarrow$ `true`) и обновляет `status` (`"todo"` $\leftrightarrow$ `"done"`).
3. **Добавление тегов (`add_tags` или `action: "add_tag"`):** добавляет теги, если они еще не назначены.
4. **Замена фрагмента текста (`action: "replace_text"`):** находит целевой фрагмент `target` во всех текстовых блоках страницы и заменяет его на `content`.

#### Пример ответа
```json
{
  "success": true,
  "id": "<capsule-native-7f31a-1792844500112@vault.local>",
  "file_name": "capsule_native_7f31a.eml",
  "subject": "План тестирования",
  "blocks_count": 4,
  "tags": ["testing", "mcp", "verified"],
  "operations": [
    "Appended 1 paragraph block",
    "Toggled task 'Написать юнит-тесты' -> done ([x])",
    "Added tags: verified"
  ]
}
```

---

### 4.7. `capsule_backlinks`

Получение графа обратных связей заметки (страницы, которые ссылаются на данную через синтаксис `[[Wiki link]]` или явный `[[<Message-ID>]]`).

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Message-ID, имя файла или заголовок целевой капсулы"
    }
  },
  "required": ["id"]
}
```

#### Пример ответа
```json
{
  "target_id": "<capsule-native-9e246-1792844358214@vault.local>",
  "target_subject": "Distributed Consensus",
  "count": 1,
  "backlinks": [
    {
      "id": "<capsule-native-9e247-1792844358215@vault.local>",
      "subject": "Replication Architecture",
      "file_name": "capsule_native_9e247.eml",
      "date": "Sat, 24 Oct 2026 15:10:00 +0300",
      "tags": ["architecture"]
    }
  ]
}
```

---

### 4.8. `capsule_related`

Семантический подбор страниц хранилища, близких по смыслу, на основе косинусного сходства векторов TF-IDF (`search::related`). Не требует нейросетевых эмбеддингов, работает мгновенно и локально.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Message-ID, заголовок или имя файла исходной капсулы"
    },
    "limit": {
      "type": "integer",
      "description": "Максимальное количество похожих страниц (по умолчанию: 5)"
    },
    "min_score": {
      "type": "number",
      "description": "Порог минимального сходства от 0.0 до 1.0 (по умолчанию: 0.01)"
    }
  },
  "required": ["id"]
}
```

#### Пример ответа
```json
{
  "target_id": "<capsule-native-9e246-1792844358214@vault.local>",
  "target_subject": "Distributed Consensus",
  "count": 1,
  "related": [
    {
      "id": "<capsule-native-9e247-1792844358215@vault.local>",
      "subject": "Replication Architecture",
      "similarity": 0.042,
      "file_name": "capsule_native_9e247.eml",
      "tags": ["architecture"]
    }
  ]
}
```

---

### 4.9. `capsule_tasks`

Глобальное сканирование, фильтрация и аудит всех задач и чекбоксов (`todo_list`) по всему хранилищу заметок.

#### Схема параметров (`inputSchema`)
```json
{
  "type": "object",
  "properties": {
    "status": {
      "type": "string",
      "enum": ["all", "open", "todo", "doing", "done"],
      "description": "Фильтр по статусу задачи (по умолчанию: 'all')"
    },
    "due": {
      "type": "string",
      "enum": ["all", "overdue", "today", "week", "upcoming", "none"],
      "description": "Фильтр по дедлайну (overdue — просрочено, upcoming — сегодня и в будущем)"
    },
    "priority": {
      "type": "string",
      "enum": ["all", "low", "normal", "high"],
      "description": "Фильтр по приоритету задачи"
    },
    "capsule_id": {
      "type": "string",
      "description": "Выбрать задачи только конкретной капсулы (Message-ID)"
    },
    "tag": {
      "type": "string",
      "description": "Выбрать задачи из капсул с указанным тегом"
    },
    "query": {
      "type": "string",
      "description": "Поисковая строка по тексту задачи или заголовку капсулы"
    }
  }
}
```

#### Пример ответа
```json
{
  "total": 2,
  "tasks": [
    {
      "id": "todo_0",
      "text": "Подготовить релизный чеклист",
      "done": false,
      "status": "todo",
      "due": "2026-10-24",
      "start": null,
      "priority": "high",
      "blocked_by": [],
      "capsule_id": "<capsule-native-7f31a-1792844500112@vault.local>",
      "capsule_subject": "План релиза",
      "capsule_tags": ["release"],
      "block_id": "blk_native_todo_list_12345",
      "block_title": "Задачи на день"
    },
    {
      "id": "todo_1",
      "text": "Прогнать тесты на macOS",
      "done": true,
      "status": "done",
      "due": null,
      "start": null,
      "priority": "normal",
      "blocked_by": [],
      "capsule_id": "<capsule-native-7f31a-1792844500112@vault.local>",
      "capsule_subject": "План релиза",
      "capsule_tags": ["release"],
      "block_id": "blk_native_todo_list_12345",
      "block_title": "Задачи на день"
    }
  ]
}
```

---

## 5. Тестирование и верификация

### Автоматические тесты
Для проверки всех обработчиков MCP запускается тест модуля `mcp`:

```powershell
# Строго с ограничением потоков (-j 1) в соответствии с правилами проекта
cargo test --manifest-path native/Cargo.toml -j 1 -- mcp
```

В набор тестов входят:
* `initialize_and_ping_flow` — протокольное рукопожатие и пинг.
* `tools_list_and_create_capsule` — листинг инструментов, создание страницы, поиск Okapi BM25 и чтение.
* `capsule_patch_flow` — добавление текста, точечная замена `replace_text`, переключение задачи `toggle_task`, добавление тегов и верификация на диске.
* `capsule_backlinks_and_related_flow` — проверка извлечения входящих ссылок и вычисления TF-IDF семантической близости.
* `capsule_tasks_flow` — сбор задач, фильтрация по статусам (`open`, `done`, `all`).

### Ручная проверка через интерактивный CLI
Отправка тестового JSON-RPC запроса в запущенный бинарник:

```powershell
$req = '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
$req | cargo run --manifest-path native/Cargo.toml -j 1 -- --mcp
```
