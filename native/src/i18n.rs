//! Localization for the native prototype.
//!
//! The web app keeps Russian as the source dictionary and English as the
//! mirror. This module does the same with a single table: every key carries
//! both strings, so parity is structural — a key without a translation does
//! not compile.
//!
//! The chosen locale is persisted outside the vault (`native-settings.json`),
//! because the vault itself stays pure `.eml`: no app state is allowed to leak
//! into the knowledge folder.

use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Ru,
    En,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::Ru => "ru",
            Locale::En => "en",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Locale::Ru => "RU",
            Locale::En => "EN",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Locale::Ru => Locale::En,
            Locale::En => Locale::Ru,
        }
    }

    /// Saved preference, else the OS language, else Russian.
    pub fn detect() -> Self {
        if let Some(saved) = read_settings().and_then(|raw| match raw.trim() {
            "ru" => Some(Locale::Ru),
            "en" => Some(Locale::En),
            _ => None,
        }) {
            return saved;
        }
        if let Ok(lang) = std::env::var("LANG") {
            if lang.to_lowercase().starts_with("ru") {
                return Locale::Ru;
            }
            return Locale::En;
        }
        Locale::Ru
    }

    pub fn persist(self) {
        let Some(dir) = settings_dir() else {
            return;
        };
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join("native-settings.json"), format!("{}\n", self.code()));
    }
}

fn settings_dir() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Some(PathBuf::from(appdata).join("com.capsuleos.app"));
    }
    if let Ok(home) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(home).join("capsuleos"));
    }
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".config").join("capsuleos"))
}

fn read_settings() -> Option<String> {
    let dir = settings_dir()?;
    fs::read_to_string(dir.join("native-settings.json")).ok()
}

macro_rules! strings {
    ($($key:ident => ($ru:literal, $en:literal)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key {
            $($key),*
        }

        impl Key {
            pub fn ru(self) -> &'static str {
                match self {
                    $(Key::$key => $ru),*
                }
            }

            pub fn en(self) -> &'static str {
                match self {
                    $(Key::$key => $en),*
                }
            }

            pub fn get(self, locale: Locale) -> &'static str {
                match locale {
                    Locale::Ru => self.ru(),
                    Locale::En => self.en(),
                }
            }

            /// Every key in declaration order — used by purism tests.
            pub fn all_keys() -> &'static [Key] {
                &[$(Key::$key),*]
            }
        }
    };
}

strings! {
    // Shell
    AppName => ("CapsuleCraft", "CapsuleCraft"),
    NativeBadge => ("NATIVE", "NATIVE"),
    Pages => ("Страницы", "Pages"),
    NewPage => ("Новая страница", "New page"),
    Vault => ("Хранилище", "Vault"),
    NoPageSelected => ("Страница не выбрана", "No page selected"),
    SearchPlaceholder => ("Поиск по хранилищу", "Search the vault"),
    NoMatches => ("Ничего не найдено", "No pages match"),
    Results => ("найдено", "found"),
    ClearSearch => ("Сбросить поиск", "Clear search"),
    ChangeFolder => ("Сменить папку…", "Change folder…"),
    ZeroDb => ("Zero-DB хранилище", "Zero-DB vault"),
    Watching => ("папка под наблюдением", "folder watched"),
    PageCount => ("стр. .eml", ".eml pages"),
    PageOne => ("страница", "page"),
    PageFew => ("страницы", "pages"),
    PageMany => ("страниц", "pages"),
    ReadOnlyCount => ("только чтение", "read-only"),
    Saved => ("Сохранено", "Saved"),
    SavedAt => ("Сохранено", "Saved"),
    Unsaved => ("Не сохранено", "Unsaved"),
    SaveFailed => ("Ошибка сохранения", "Save failed"),
    Reload => ("Перечитать с диска", "Reload from disk"),
    SaveNow => ("Сохранить (Ctrl+S)", "Save now (Ctrl+S)"),
    AllSaved => ("Всё сохранено", "All changes saved"),
    NewSubpage => ("Подстраница", "Subpage"),
    NewSubpageHint => ("Создать страницу внутри этой", "Create a page under this one"),
    Info => ("Инфо", "Info"),
    MimeAndPage => ("MIME и свойства страницы", "MIME and page details"),
    Untitled => ("Без названия", "Untitled"),
    Blocks => ("блоков", "blocks"),
    BlockOne => ("блок", "block"),
    BlockFew => ("блока", "block"),
    BlockMany => ("блоков", "blocks"),
    ThreadOne => ("тред", "thread"),
    ThreadFew => ("треда", "thread"),
    ThreadMany => ("тредов", "threads"),
    ReplyOne => ("ответ", "reply"),
    ReplyFew => ("ответа", "reply"),
    ReplyMany => ("ответов", "replies"),
    ImportedEmail => ("импортированное письмо", "imported email"),
    ReadOnly => ("ТОЛЬКО ЧТЕНИЕ", "READ-ONLY"),
    Attachment => ("вложение", "attachment"),
    Attachments => ("вложений", "attachments"),
    VaultOpened => ("Хранилище открыто", "Vault opened"),
    Reloaded => ("Перечитано", "Reloaded"),
    NewPageDone => ("Новая страница", "New page"),
    UntitledPage => ("Без названия", "Untitled"),

    // Navigation
    NavPage => ("Страница", "Page"),
    NavPlanning => ("Задачи", "Planning"),
    NavGraph => ("Граф", "Graph"),
    NavThreads => ("Треды", "Threads"),
    NavMime => ("MIME", "MIME"),
    NavPreview => ("Письмо", "Email"),
    PaletteHint => ("Команды и страницы", "Commands and pages"),
    PalettePlaceholder => ("Найти страницу или команду…", "Find a page or a command…"),
    PaletteActions => ("Действия", "Actions"),
    PaletteNavigation => ("Навигация", "Navigation"),
    PalettePages => ("Страницы", "Pages"),
    PaletteEmpty => ("Ничего не найдено", "Nothing matches"),
    CommandNewPage => ("Создать страницу", "Create page"),
    CommandNewPageHint => ("Пустая страница в корне", "Empty page at the root"),
    CommandUndo => ("Отменить", "Undo"),
    CommandRedo => ("Повторить", "Redo"),
    CommandExportPage => ("Экспорт .eml", "Export .eml"),
    CommandExportMarkdown => ("Экспорт Markdown", "Export Markdown"),
    CommandExportVault => ("Экспорт хранилища", "Export vault"),
    CommandImport => ("Импорт .eml", "Import .eml"),
    CommandImportObsidian => ("Импорт Obsidian / Markdown…", "Import Obsidian / Markdown…"),
    ImportObsidianTitle => ("Выберите папку хранилища Obsidian", "Select Obsidian Vault Folder"),
    ObsidianImportSummary => ("Импортировано из Obsidian", "Imported from Obsidian"),
    CommandLocale => ("Переключить язык", "Switch language"),
    CommandZenMode => ("Режим фокуса", "Zen mode"),
    ZenModeEntered => ("Режим фокуса (F11 / Esc для выхода)", "Zen mode (F11 / Esc to exit)"),
    ZenModeExited => ("Обычный режим", "Normal mode"),
    CommandToggleSidebar => ("Боковая панель", "Toggle sidebar"),
    CommandZoomIn => ("Увеличить масштаб", "Zoom in"),
    CommandZoomOut => ("Уменьшить масштаб", "Zoom out"),
    CommandResetZoom => ("Сбросить масштаб", "Reset zoom"),
    CommandWideCanvas => ("Широкий холст", "Toggle wide canvas"),
    CommandToggleIconStyle => ("Сменить стиль иконки (Маскот / Документ)", "Toggle app icon style (Mascot / Document)"),
    IconStyleToast => ("Стиль иконки изменен (окно обновится при перезапуске)", "App icon style changed (applies to window on restart)"),
    ZoomLevel => ("Масштаб", "Zoom"),
    UndoEmpty => ("История пуста", "Nothing to undo"),
    RedoEmpty => ("Нечего повторять", "Nothing to redo"),
    Undone => ("Отменено", "Undone"),
    Redone => ("Повторено", "Redone"),

    // Editor
    AddBlock => ("Блок", "Block"),
    AddBlockHint => ("Добавить блок", "Add a block"),
    BlockHeading => ("Заголовок", "Heading"),
    BlockParagraph => ("Абзац", "Paragraph"),
    BlockTodo => ("Список задач", "Task list"),
    BlockCallout => ("Врезка", "Callout"),
    BlockCode => ("Код", "Code"),
    BlockGrid => ("Таблица данных", "Data table"),
    BlockAudio => ("Аудиозаметка", "Audio note"),
    BlockCanvas => ("Доска", "Whiteboard"),
    BlockAi => ("Контраргумент", "Counter-argument"),
    MoveUp => ("Выше", "Move up"),
    MoveDown => ("Ниже", "Move down"),
    DeleteBlock => ("Удалить блок", "Delete block"),
    HeadingLevel => ("Уровень", "Heading level"),
    TurnInto => ("Превратить в…", "Turn into…"),
    EditNote => ("Редактировать", "Edit note"),
    PlaceholderParagraph => ("Пишите. Markdown-lite: **жирный**, *курсив*, `код`, [[ссылка]].", "Write. Markdown-lite: **bold**, *italic*, `code`, [[link]]."),
    PlaceholderHeading => ("Заголовок", "Heading"),
    PlaceholderCode => ("Код без выполнения", "Code, never executed"),
    PlaceholderItem => ("Новая задача", "New task"),
    Due => ("Срок", "Due"),
    Start => ("Начало", "Start"),
    Priority => ("Приоритет", "Priority"),
    PriorityLow => ("низкий", "low"),
    PriorityNormal => ("обычный", "normal"),
    PriorityHigh => ("высокий", "high"),
    Status => ("Статус", "Status"),
    StatusTodo => ("К выполнению", "To do"),
    StatusDoing => ("В работе", "In progress"),
    StatusDone => ("Готово", "Done"),
    Blocked => ("Заблокировано", "Blocked"),
    BlockedBy => ("Зависит от", "Blocked by"),
    Remove => ("Удалить", "Remove"),
    NoDependency => ("Нет зависимостей", "No dependencies"),
    Language => ("Язык", "Language"),
    Copy => ("Копировать", "Copy"),
    Copied => ("Скопировано", "Copied"),
    NotExecuted => ("Нативный прототип не исполняет код. Текст сохраняется в .eml.", "The native prototype never executes code. Source is stored in the .eml."),
    RunUnavailable => ("Запуск недоступен", "Run unavailable"),
    CodeRun => ("Запустить", "Run"),
    CodeExecuted => ("Выполнено", "Executed"),
    CodeExecError => ("Ошибка выполнения", "Execution error"),
    CodeUnsupported => (
        "интерпретатор не входит в offline-сборку (zero-dependency). Код сохранён в .eml как текст. Исполняемые языки: JavaScript, JSON.",
        "has no interpreter in the offline build (zero-dependency). The code is stored in the .eml as text. Executable languages: JavaScript, JSON."
    ),
    AudioNoFile => ("Аудиофайл не приложен", "No audio file attached"),
    AudioAttach => ("Приложить аудио…", "Attach audio…"),
    AudioPlayback => ("Воспроизведение в нативном прототипе не реализовано.", "Playback is not implemented in the native prototype."),
    AudioPlay => ("Играть", "Play"),
    AudioPause => ("Пауза", "Pause"),
    AudioStop => ("Стоп", "Stop"),
    AudioPlaying => ("Играет…", "Playing…"),
    AudioPaused => ("На паузе", "Paused"),
    AudioUnsupportedFmt => (
        "Веб-запись webm/opus не декодируется офлайн-плеером. Приложите mp3/wav/ogg/flac.",
        "Browser-recorded webm/opus cannot be decoded by the offline player. Attach mp3/wav/ogg/flac instead."
    ),
    AudioDeviceFail => ("Аудиоустройство недоступно.", "No audio output device."),
    Transcript => ("Расшифровка", "Transcript"),
    CanvasEmpty => ("На доске пока нет элементов", "The board has no elements yet"),
    CanvasAddSticky => ("Стикер", "Sticky"),
    CanvasAddRect => ("Прямоугольник", "Rectangle"),
    CanvasAddText => ("Текст", "Text"),
    CanvasReset => ("Сброс вида", "Reset view"),
    CanvasHint => ("Перетаскивание и масштаб колесом", "Drag to pan, wheel to zoom"),
    Confidence => ("уверенность", "confidence"),
    SourceClaim => ("Исходное утверждение", "Source claim"),

    // Wiki links
    LinksTitle => ("Ссылки", "Links"),
    NoLinks => ("Нет ссылок. Заключите название страницы в [[двойные скобки]].", "No links yet. Wrap a page title in [[double brackets]]."),
    Backlinks => ("Обратные ссылки", "Linked from"),
    Related => ("Похожие по смыслу", "Related by meaning"),
    OpenLinkedPage => ("Открыть страницу", "Open the page"),
    CreateLinkedPage => ("Создать эту страницу", "Create this page"),
    PageCreated => ("Страница создана", "Page created"),
    LinksUpdated => ("Ссылки обновлены", "Links updated"),

    // Grid formulas (boa)
    FormulaHint => ("=SUM(A1:A5) · =AVG · =MIN · =MAX · =COUNT", "=SUM(A1:A5) · =AVG · =MIN · =MAX · =COUNT"),

    // OpenPGP capsule encryption (rPGP)
    EncryptCapsuleCmd => ("Зашифровать капсулу (.eml.asc)", "Encrypt capsule (.eml.asc)"),
    DecryptEmlCmd => ("Расшифровать .eml.asc в хранилище", "Decrypt .eml.asc into the vault"),
    EncryptTitle => ("Шифрование OpenPGP", "OpenPGP encryption"),
    DecryptTitle => ("Расшифрование OpenPGP", "OpenPGP decryption"),
    EncryptSource => ("Источник", "Source"),
    Passphrase => ("Парольная фраза", "Passphrase"),
    RepeatPassphrase => ("Повторите парольную фразу", "Repeat passphrase"),
    PassMismatch => ("Парольные фразы не совпадают", "Passphrases do not match"),
    PassEmpty => ("Пустая парольная фраза", "Empty passphrase"),
    EncryptNow => ("Зашифровать", "Encrypt"),
    DecryptNow => ("Расшифровать", "Decrypt"),
    EncryptedDone => ("Зашифровано", "Encrypted"),
    DecryptDone => ("Расшифровано и добавлено в хранилище", "Decrypted and added to the vault"),
    CryptoFailed => ("Не удалось расшифровать: неверная парольная фраза или повреждённый файл", "Decryption failed: wrong passphrase or corrupted file"),
    EncryptHint => ("AES-256 · SEIPD v1 · офлайн rPGP, без сети", "AES-256 · SEIPD v1 · offline rPGP, no network"),

    // Planning
    PlanningTitle => ("Планирование", "Planning"),
    Agenda => ("Повестка", "Agenda"),
    Kanban => ("Канбан", "Kanban"),
    Gantt => ("Гантт", "Gantt"),
    Calendar => ("Календарь", "Calendar"),
    Overdue => ("Просрочено", "Overdue"),
    Today => ("Сегодня", "Today"),
    Tomorrow => ("Завтра", "Tomorrow"),
    ThisWeek => ("На этой неделе", "This week"),
    Later => ("Позже", "Later"),
    NoDate => ("Без даты", "No date"),
    Done => ("Выполнено", "Done"),
    All => ("Все", "All"),
    Open => ("Открытые", "Open"),
    Filters => ("Фильтры", "Filters"),
    ClearFilters => ("Сбросить", "Clear"),
    FilterText => ("Поиск задач", "Search tasks"),
    FilterCapsule => ("Капсула", "Capsule"),
    FilterTag => ("Тег", "Tag"),
    QuickAdd => ("Быстрое добавление", "Quick add"),
    AddTask => ("Добавить", "Add"),
    TasksTotal => ("всего", "total"),
    TasksOpen => ("открыто", "open"),
    TasksDone => ("готово", "done"),
    TasksOverdue => ("просрочено", "overdue"),
    ExportIcs => ("Экспорт .ics", "Export .ics"),
    EmptyPlanning => ("В хранилище нет задач. Добавьте список задач на любой странице.", "No tasks in this vault yet. Add a task list to any page."),
    EmptyFiltered => ("Под фильтры ничего не подходит.", "Nothing matches these filters."),
    TaskAdded => ("Задача добавлена", "Task added"),
    InboxSubject => ("Входящие задачи", "Inbox tasks"),
    TodayIs => ("сегодня", "today"),
    SpanDays => ("дн.", "d"),
    BlockedShort => ("зависит", "blocked"),

    // Graph / threads
    GraphTitle => ("Граф связей", "Relationship graph"),
    GraphHint => ("Колесо — масштаб, перетаскивание — панорама", "Wheel to zoom, drag to pan"),
    Fit => ("Вписать", "Fit"),
    TagFilter => ("Тег", "Tag"),
    OpenInEditor => ("Открыть в редакторе", "Open in editor"),
    Nodes => ("узлов", "nodes"),
    Edges => ("связей", "edges"),
    PageParent => ("страница", "page"),
    Reply => ("ответ", "reply"),
    Reference => ("ссылка", "reference"),
    TagCluster => ("тег", "tag"),
    ThreadsTitle => ("Треды мыслей", "Thought threads"),
    ThreadsEmpty => ("Нет цепочек ответов", "No reply chains"),
    ThreadsOnly => ("Треды", "Threads"),
    AllPages => ("Все страницы", "All pages"),
    CreateReply => ("Ответить", "Reply"),
    ReplyCreated => ("Ответ создан", "Reply created"),
    RootThread => ("Начало треда", "Thread root"),

    // MIME / preview
    MimeTitle => ("Сырой MIME", "Raw MIME"),
    MimeHint => ("Ровно те байты, что лежат в .eml", "Exactly the bytes stored in the .eml"),
    ExportEml => ("Скачать .eml", "Download .eml"),
    ExportMarkdown => ("Экспорт Markdown", "Export Markdown"),
    CopyMime => ("Копировать MIME", "Copy MIME"),
    Bytes => ("байт", "bytes"),
    Headers => ("Заголовки", "Headers"),
    Body => ("Тело", "Body"),
    ExternalPreview => ("Импортированное письмо", "Imported email"),
    ExternalReadOnlyNotice => ("Файл не создан CapsuleCraft. Нативная версия показывает текст письма и никогда его не перезаписывает.", "This file was not written by CapsuleCraft. The native app renders its text and never rewrites it."),
    HtmlRenderingNotice => ("Упрощённый рендер: теги и ссылки разбираются, CSS не применяется.", "Simplified rendering: tags and links are parsed, CSS is not applied."),
    NoBody => ("Тело письма пустое или не распознано", "The message body is empty or unrecognised"),
    PlainPart => ("text/plain", "text/plain"),
    HtmlPart => ("text/html", "text/html"),
    OpenLink => ("Открыть ссылку", "Open link"),

    // Attachments / pages
    AttachmentsTitle => ("Вложения", "Attachments"),
    SaveAs => ("Сохранить как…", "Save as…"),
    AddAttachment => ("Добавить файл…", "Add file…"),
    RemoveAttachment => ("Удалить вложение", "Remove attachment"),
    AttachmentSaved => ("Вложение сохранено", "Attachment saved"),
    AttachmentAdded => ("Вложение добавлено", "Attachment added"),
    AttachmentRemoved => ("Вложение удалено", "Attachment removed"),
    AttachFailed => ("Не удалось добавить вложение", "Could not add the attachment"),
    ImportEml => ("Импорт .eml…", "Import .eml…"),
    Imported => ("Импортировано", "Imported"),
    DropHint => ("Отпустите файлы .eml, чтобы добавить их в хранилище", "Drop .eml files to add them to the vault"),
    SkippedFiles => ("пропущено", "skipped"),
    ExportVault => ("Экспорт хранилища…", "Export vault…"),
    VaultExported => ("Хранилище скопировано", "Vault exported"),
    DeletePage => ("Удалить страницу", "Delete page"),
    DeletePageConfirm => ("Удалить этот .eml с диска?", "Delete this .eml from disk?"),
    Delete => ("Удалить", "Delete"),
    Cancel => ("Отмена", "Cancel"),
    PageDeleted => ("Страница удалена", "Page deleted"),
    Tags => ("Теги", "Tags"),
    AddTag => ("Добавить тег", "Add tag"),
    Parent => ("Родитель", "Parent"),
    NoParent => ("Корень", "Root"),
    Reparent => ("Переместить", "Move"),
    CycleRejected => ("Нельзя создать цикл в дереве", "That would create a page cycle"),
    Reparented => ("Страница перемещена", "Page moved"),
    StatusActive => ("Активна", "Active"),
    StatusArchived => ("Архив", "Archived"),
    StatusDraft => ("Черновик", "Draft"),
    StatusSynthesis => ("Синтез", "Synthesis"),

    // Co-Think (offline heuristics)
    CoThink => ("Co-Think (офлайн)", "Co-Think (offline)"),
    CoThinkHint => ("Локальная эвристика по содержимому страницы", "Local heuristics over the page content"),
    Critique => ("Критика", "Critique"),
    Synthesis => ("Синтез", "Synthesis"),
    CounterArg => ("Контраргумент", "Counter-argument"),
    DataInsight => ("Инсайт по данным", "Data insight"),
    CoThinkWorked => ("Локальный анализ добавлен в страницу", "Local analysis added to the page"),
    HeuristicNotice => ("Это детерминированная локальная эвристика, а не модель. Сеть не используется.", "These are deterministic local heuristics, not a model. No network is used."),
    CoThinkTitle => ("Офлайн-помощник Co-Thinker", "Offline Assistant Co-Thinker"),
    CoThinkSubtitle => ("Детерминированный когнитивный анализ содержимого без обращения в сеть", "Deterministic cognitive analysis of content with zero network calls"),
    CoThinkRun => ("Запустить Co-Think", "Run Co-Think"),
    CoThinkFocus => ("Фокус анализа (опционально):", "Analysis focus (optional):"),
    CoThinkFocusPlaceholder => ("Например: риски масштабирования, сроки, бюджет...", "E.g. scaling risks, deadlines, budget..."),
    CritiqueDesc => ("Поиск рисков, проверка гипотез и когнитивный линтер", "Risk detection, claim testing and thought linter"),
    SynthesisDesc => ("Конструктивный синтез и следующие практические шаги", "Constructive synthesis and concrete next steps"),
    CounterArgDesc => ("Создать капсулу-ответ с контраргументами в тред", "Create a counter-argument reply capsule in thread"),
    DataInsightDesc => ("Честный OLS-тренд и корреляция по CSV-вложениям", "Honest OLS trend and correlation over CSV attachments"),
    ReplyCapsuleCreated => ("Создана капсула с контраргументами в треде", "Counter-argument reply capsule created in thread"),

    // MCP Integration
    ConnectMcp => ("Подключить ИИ-агента (MCP)", "Connect AI Agent (MCP)"),
    McpTitle => ("Интеграция с ИИ-агентами (Model Context Protocol)", "AI Agent Integration (Model Context Protocol)"),
    McpSubtitle => ("CapsuleCraft работает как внешняя память (Second Brain) для Claude Desktop, Antigravity, Claude Code и Cursor.", "CapsuleCraft acts as an external Second Brain for Claude Desktop, Antigravity, Claude Code, and Cursor."),
    CopyConfig => ("Скопировать конфиг", "Copy configuration"),
    ConfigCopied => ("Конфигурация скопирована в буфер обмена!", "Configuration copied to clipboard!"),
    McpTabAntigravity => ("Antigravity", "Antigravity"),
    McpTabClaudeDesktop => ("Claude Desktop", "Claude Desktop"),
    McpTabClaudeCode => ("Claude Code", "Claude Code"),
    McpTabCursor => ("Cursor", "Cursor"),

    // Misc
    Yes => ("Да", "Yes"),
    No => ("Нет", "No"),
    Close => ("Закрыть", "Close"),
    Keyword => ("Слово", "Keyword"),
    Page => ("Страница", "Page"),
    EmptyVaultTitle => ("Хранилище пусто", "The vault is empty"),
    EmptyVaultBody => ("Одна страница — один .eml в локальной папке. Ни базы данных, ни сервера.", "One page is one .eml in a local folder. No database, no server."),
    CreateFirstPage => ("Создать первую страницу", "Create the first page"),
    SlashHint => ("«/» в пустом абзаце открывает блоки", "Type “/” in an empty paragraph for blocks"),
    // FAQ modal
    Faq => ("FAQ", "FAQ"),
    FaqTitle => ("CapsuleCraft — FAQ", "CapsuleCraft — FAQ"),
    FaqSubtitle => ("Быстрые ответы о хранении, поиске и переносе данных.", "Quick answers about storage, search and moving your data."),
    FaqQ1 => ("Что такое CapsuleCraft в двух словах?", "What is CapsuleCraft, in two sentences?"),
    FaqA1 => ("Локальный рабочий стол для мышления. Каждая мысль — настоящий RFC 5322/2045 файл .eml: блочный редактор, треды как в почте, граф связей и офлайн Co-Thinker. Никакой базы данных, сервера и облака — папка .eml-файлов и есть вся база знаний.", "A local workbench for thinking. Every thought is a real RFC 5322/2045 .eml file: block editor, mail-style threads, link graph and an offline Co-Thinker. No database, server or cloud — a folder of .eml files is the entire knowledge base."),
    FaqQ2 => ("Где физически хранятся мои заметки?", "Where are my notes stored, physically?"),
    FaqA2 => ("В папке, которую вы выбрали; по умолчанию — папка данных приложения (~/.local/share/com.capsuleos.app/vault в Linux, %APPDATA%\\com.capsuleos.app в Windows). Одна страница — один .eml, vault.json — только одноразовый кэш индекса, папка всегда главнее.", "In the folder you chose; by default the app-data directory (~/.local/share/com.capsuleos.app/vault on Linux, %APPDATA%\\com.capsuleos.app on Windows). One page is one .eml; vault.json is just a disposable index cache — the folder always wins."),
    FaqQ3 => ("Почему .eml, а не база данных?", "Why .eml instead of a database?"),
    FaqA3 => (".eml — открытый стандарт, который старше большинства NoSQL-мод: любой почтовый клиент прочтёт файл, git продиффит, rsync зарезервирует. Миграций не бывает, вендорной базы не существует. Приложение можно удалить — файлы останутся и откроются заново.", ".eml is an open standard older than most NoSQL fashions: any mail client can read the file, git can diff it, rsync can back it up. No migrations ever, no vendor database. Delete the app — the files survive and reopen as-is."),
    FaqQ4 => ("Как работают треды и вики-ссылки?", "How do threads and wiki links work?"),
    FaqA4 => ("Ответ на мысль — дочерний .eml с заголовками In-Reply-To/References, ровно как в электронной почте, поэтому ветки видны в виде тредов. [[вики-ссылки]] между страницами дают обратные ссылки (backlinks) и граф связей; переименование страницы обновляет все ссылки.", "A reply is a child .eml carrying In-Reply-To/References headers — exactly like email, so chains render as threads. [[Wiki links]] between pages produce backlinks and the link graph; renaming a page rewrites its links."),
    FaqQ5 => ("Как устроен поиск?", "How does search work?"),
    FaqA5 => ("Okapi BM25 по всему хранилищу: sidebar показывает баллы релевантности и выдержку с подсветкой первого совпадения. «Похожие по смыслу» ранжируют страницы tf-idf-косинусом по тому же индексу. Всё считается локально.", "Okapi BM25 over the whole vault: the sidebar ranks pages with relevance scores and a highlighted excerpt around the first hit. “Related by meaning” ranks pages by tf-idf cosine over the same index. Everything is computed locally."),
    FaqQ6 => ("Как перенести базу на другой компьютер?", "How do I move my vault to another machine?"),
    FaqA6 => ("Скопируйте папку vault — это и есть вся база. Или нажмите Export Vault в сайдбаре (ZIP со всеми .eml). Подойдёт и синхронзация по git: файлы текстовые и диффятся построчно.", "Copy the vault folder — that is the entire database. Or hit Export Vault in the sidebar (a ZIP of all .eml files). A git sync works too: the files are plain text and diff line by line."),
    FaqQ7 => ("Как импортировать письма (.eml)?", "How do I import emails (.eml)?"),
    FaqA7 => ("Перетащите .eml-файлы в окно приложения или откройте импорт по кнопке в сайдбаре. Файлы копируются байт-в-байт, заголовки From/Subject/In-Reply-To сохраняются, поэтому импорт письма с ответом сразу встраивается в тред.", "Drop the .eml files onto the app window or use the import button in the sidebar. Files are copied byte-for-byte with From/Subject/In-Reply-To headers kept, so a reply email joins its thread on arrival."),
    FaqQ8 => ("Что будет, если удалить или переименовать файл?", "What happens if I delete or rename a file?"),
    FaqA8 => ("Удалите .eml — капсула исчезнет; переименуете — она появится под новым именем: хранилище читается прямо с диска при каждом старте и обновляется при изменении файлов. Это обычная папка, поэтому резервная копия = копия папки.", "Delete an .eml — the capsule is gone; rename it — it reappears under the new name: the vault is read directly from disk at every start and refreshed as files change. It is an ordinary folder, so a backup is just a folder copy."),
    FaqCloseShort => ("Понятно", "Got it"),
    NoTasks => ("Пока нет задач.", "No tasks yet."),
    AddTaskShort => ("Задача", "Task"),
    TaskStatus => ("Статус задачи", "Task status"),
    DeleteTask => ("Удалить задачу", "Delete task"),
    BlockPreserved => ("Сохранён", "Preserved"),
    BlockPreservedHint => ("Этот тип блока не редактируется в нативном прототипе: он остаётся в canvas_state.json и открывается в веб-версии.", "This block type is not editable in the native prototype: it stays in canvas_state.json and opens in the web app."),
    CodeOutput => ("Вывод", "Output"),
    DataGridMissing => ("Источник данных не найден среди вложений", "The data source is not among the attachments"),
    CanvasDelete => ("Удалить элемент", "Delete element"),
    AttachAudio => ("Прикрепить аудио", "Attach audio"),
    NoPlayback => ("Воспроизведение в нативном прототипе не реализовано.", "Playback is not implemented in the native prototype."),
    EmptyAttachments => ("Вложений нет", "No attachments"),
    AddColumn => ("Добавить столбец", "Add column"),
    AddRow => ("Добавить строку", "Add row"),
    RenameField => ("Название", "Name"),

    // Drag-and-drop hierarchy
    MakeChildOf => ("Сделать дочерней для", "Make child of"),
    MoveToRoot => ("Переместить в корень", "Move to root"),
    CannotDropInsideSelf => ("Нельзя переместить внутрь собственного поддерева", "Cannot move inside its own subtree"),

    // IMAP Sync
    ImapSyncTitle => ("Синхронизация почты (IMAP)", "Email Sync (IMAP)"),
    ImapSyncDesc => ("Синхронизация капсул через почтовый ящик по протоколу IMAP.", "Synchronize capsules via an IMAP mailbox."),
    ImapHost => ("IMAP сервер", "IMAP Host"),
    ImapPort => ("Порт", "Port"),
    ImapUsername => ("Логин / Email", "Username / Email"),
    ImapPassword => ("Пароль приложения", "App Password"),
    ImapFolder => ("Папка на сервере", "Mailbox Folder"),
    ImapEnable => ("Включить синхронизацию", "Enable synchronization"),
    ImapTestConnection => ("Проверить связь", "Test Connection"),
    ImapSyncNow => ("Синхронизировать сейчас", "Sync Now"),
    ImapSaveSettings => ("Сохранить настройки", "Save Settings"),
    ImapTesting => ("Проверка...", "Testing..."),
    ImapSyncing => ("Синхронизация...", "Syncing..."),
    ImapSynced => ("Синхронизировано", "Synced"),
    ImapDisabled => ("IMAP выкл", "IMAP off"),
    ImapSuccessToast => ("Синхронизация успешно выполнена", "Sync completed successfully"),
    ImapErrorToast => ("Ошибка синхронизации", "Sync error"),
}

pub fn t(locale: Locale, key: Key) -> &'static str {
    key.get(locale)
}

/// "1 page / 3 pages" with the right plural form for the locale.
///
/// English has two forms; Russian needs three (`1 блок`, `2 блока`,
/// `5 блоков`), so callers always pass all three and English simply reuses
/// `one` for the singular and `many` for everything else.
pub fn plural(locale: Locale, count: usize, one: Key, few: Key, many: Key) -> String {
    let key = match locale {
        Locale::En => {
            if count == 1 {
                one
            } else {
                many
            }
        }
        Locale::Ru => {
            let last_two = count % 100;
            let last = count % 10;
            if last == 1 && last_two != 11 {
                one
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                few
            } else {
                many
            }
        }
    };
    format!("{} {}", count, t(locale, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_both_languages() {
        // The macro cannot produce a missing side, but an empty string would
        // still be a silent hole in the UI.
        for key in Key::all_keys() {
            assert!(!key.ru().is_empty(), "empty RU string for {:?}", key);
            assert!(!key.en().is_empty(), "empty EN string for {:?}", key);
        }
    }

    #[test]
    fn english_strings_are_cyrillic_free() {
        // Strict EN purism: the English locale must never render Russian,
        // so no EN column may contain a single Cyrillic character.
        for key in Key::all_keys() {
            let en = key.en();
            assert!(
                !en.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
                "EN string for {:?} contains Cyrillic: {:?}",
                key,
                en
            );
        }
    }

    #[test]
    fn locale_switches() {
        assert_eq!(t(Locale::Ru, Key::Today), "Сегодня");
        assert_eq!(t(Locale::En, Key::Today), "Today");
        assert_eq!(Locale::Ru.other(), Locale::En);
    }

    #[test]
    fn russian_plurals_follow_the_three_forms() {
        let plural_blocks = |count| {
            plural(
                Locale::Ru,
                count,
                Key::BlockOne,
                Key::BlockFew,
                Key::BlockMany,
            )
        };
        assert_eq!(plural_blocks(1), "1 блок");
        assert_eq!(plural_blocks(2), "2 блока");
        assert_eq!(plural_blocks(4), "4 блока");
        assert_eq!(plural_blocks(5), "5 блоков");
        assert_eq!(plural_blocks(11), "11 блоков");
        assert_eq!(plural_blocks(21), "21 блок");
        assert_eq!(plural_blocks(22), "22 блока");
        assert_eq!(plural_blocks(112), "112 блоков");
    }

    #[test]
    fn english_plurals_have_two_forms() {
        assert_eq!(
            plural(Locale::En, 1, Key::ReplyOne, Key::ReplyFew, Key::ReplyMany),
            "1 reply"
        );
        assert_eq!(
            plural(Locale::En, 2, Key::ReplyOne, Key::ReplyFew, Key::ReplyMany),
            "2 replies"
        );
    }
}
