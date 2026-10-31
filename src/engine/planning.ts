/**
 * Planning layer — tasks collected out of every capsule's todo blocks.
 *
 * Everything here is pure and deterministic so it can be tested without a DOM.
 * Dates are stored as local calendar days (`YYYY-MM-DD`), not UTC instants:
 * "due Friday" must survive a timezone or DST change unchanged.
 */

import { CapsuleNode, TaskStatus, TodoItem, TodoPriority } from '../types';

export type TaskPriority = TodoPriority;

export interface PlannedTask {
  /** Capsule that owns the todo block. */
  capsuleId: string;
  capsuleSubject: string;
  capsuleTags: string[];
  blockId: string;
  blockTitle?: string;
  item: TodoItem;
}

export type BucketId = 'overdue' | 'today' | 'tomorrow' | 'week' | 'later' | 'none' | 'done';

export type TaskFilterStatus = 'all' | 'open' | TaskStatus;
export type TaskDueFilter = 'all' | 'overdue' | 'today' | 'week' | 'none';

export interface TaskFilters {
  /** Case-insensitive substring over task text, capsule subject, list title and tags. */
  text?: string;
  status?: TaskFilterStatus;
  priority?: TodoPriority | 'all';
  due?: TaskDueFilter;
  capsuleId?: string | null;
  tag?: string | null;
}

export const EMPTY_TASK_FILTERS: TaskFilters = {
  text: '',
  status: 'all',
  priority: 'all',
  due: 'all',
  capsuleId: null,
  tag: null,
};

export function hasActiveTaskFilters(filters: TaskFilters): boolean {
  return (
    !!filters.text?.trim() ||
    (!!filters.status && filters.status !== 'all') ||
    (!!filters.priority && filters.priority !== 'all') ||
    (!!filters.due && filters.due !== 'all') ||
    !!filters.capsuleId ||
    !!filters.tag
  );
}

export interface TaskSection {
  id: BucketId;
  title: string;
  /** Tasks that belong to this bucket, sorted by due date then priority. */
  tasks: PlannedTask[];
}

export interface TaskStats {
  total: number;
  done: number;
  open: number;
  overdue: number;
  today: number;
  completionRate: number;
}

/** Default (English) bucket titles — the UI passes localized ones from i18n. */
export const BUCKET_TITLES: Record<BucketId, string> = {
  overdue: 'Overdue',
  today: 'Today',
  tomorrow: 'Tomorrow',
  week: 'This week',
  later: 'Later',
  none: 'No due date',
  done: 'Completed',
};

const PRIORITY_WEIGHT: Record<TaskPriority, number> = { high: 0, normal: 1, low: 2 };

const pad2 = (n: number): string => String(n).padStart(2, '0');

/** Local calendar day key, e.g. `2026-10-22`. */
export function toDateKey(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/** Parse `YYYY-MM-DD` as a local midnight; returns null for malformed input. */
export function parseDateKey(key: string | undefined): Date | null {
  if (!key) return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  const day = Number(m[3]);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) {
    return null;
  }
  return date;
}

export function isDateKey(value: string | undefined): boolean {
  return parseDateKey(value) !== null;
}

/** Add (or subtract) whole calendar days, immune to DST hour shifts. */
export function addDays(key: string, days: number): string {
  const base = parseDateKey(key);
  if (!base) return key;
  return toDateKey(new Date(base.getFullYear(), base.getMonth(), base.getDate() + days));
}

/** Whole days from `from` to `to` (negative when `to` is in the past). */
export function daysBetween(from: string, to: string): number {
  const a = parseDateKey(from);
  const b = parseDateKey(to);
  if (!a || !b) return 0;
  const utcA = Date.UTC(a.getFullYear(), a.getMonth(), a.getDate());
  const utcB = Date.UTC(b.getFullYear(), b.getMonth(), b.getDate());
  return Math.round((utcB - utcA) / 86_400_000);
}

/** Six-week (42-cell) month grid starting on Monday. */
export function monthGrid(year: number, month: number): string[] {
  const first = new Date(year, month, 1);
  // getDay(): 0 = Sunday. Shift so Monday is 0.
  const offset = (first.getDay() + 6) % 7;
  const start = new Date(year, month, 1 - offset);
  const cells: string[] = [];
  for (let i = 0; i < 42; i += 1) {
    cells.push(toDateKey(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i)));
  }
  return cells;
}

export function monthLabel(year: number, month: number, locale = 'ru-RU'): string {
  return new Date(year, month, 1).toLocaleDateString(locale, { month: 'long', year: 'numeric' });
}

export function humanDate(key: string, locale = 'ru-RU'): string {
  const date = parseDateKey(key);
  if (!date) return key;
  return date.toLocaleDateString(locale, { day: 'numeric', month: 'short' });
}

/** Every todo item across the vault, with its owning capsule. */
export function collectTasks(capsules: CapsuleNode[]): PlannedTask[] {
  const tasks: PlannedTask[] = [];
  for (const capsule of capsules) {
    for (const block of capsule.blocks) {
      if (block.type !== 'todo_list') continue;
      for (const item of block.data.items || []) {
        tasks.push({
          capsuleId: capsule.messageId,
          capsuleSubject: capsule.subject,
          capsuleTags: capsule.tags || [],
          blockId: block.id,
          blockTitle: block.data.title,
          item,
        });
      }
    }
  }
  return tasks;
}

/** Distinct capsule tags present on task-owning capsules, alphabetically. */
export function taskTags(tasks: PlannedTask[]): string[] {
  const tags = new Set<string>();
  for (const task of tasks) {
    for (const tag of task.capsuleTags) tags.add(tag);
  }
  return Array.from(tags).sort((a, b) => a.localeCompare(b));
}

/**
 * Cross-project task filter. Pure and composable: every field is optional,
 * `all` / empty means "do not filter by this dimension".
 */
export function filterTasks(
  tasks: PlannedTask[],
  filters: TaskFilters,
  today: string,
): PlannedTask[] {
  const needle = (filters.text || '').trim().toLowerCase();
  const status = filters.status || 'all';
  const priority = filters.priority || 'all';
  const due = filters.due || 'all';
  const weekEnd = addDays(today, 7);

  return tasks.filter((task) => {
    if (needle) {
      const haystack = [
        task.item.text,
        task.capsuleSubject,
        task.blockTitle || '',
        task.capsuleTags.join(' '),
      ]
        .join('\n')
        .toLowerCase();
      if (!haystack.includes(needle)) return false;
    }

    const currentStatus = taskStatus(task.item);
    if (status === 'open' && currentStatus === 'done') return false;
    if (status !== 'all' && status !== 'open' && currentStatus !== status) return false;

    if (priority !== 'all' && (task.item.priority || 'normal') !== priority) return false;

    if (filters.capsuleId && task.capsuleId !== filters.capsuleId) return false;
    if (filters.tag && !task.capsuleTags.includes(filters.tag)) return false;

    if (due !== 'all') {
      const day = task.item.due;
      if (due === 'none') {
        if (isDateKey(day)) return false;
      } else if (due === 'overdue') {
        if (!isDateKey(day) || day! >= today || currentStatus === 'done') return false;
      } else if (due === 'today') {
        if (day !== today) return false;
      } else if (due === 'week') {
        if (!isDateKey(day) || day! < today || day! > weekEnd) return false;
      }
    }

    return true;
  });
}

export function bucketOf(task: PlannedTask, today: string): BucketId {
  if (taskStatus(task.item) === 'done') return 'done';
  const due = task.item.due;
  if (!isDateKey(due)) return 'none';
  const delta = daysBetween(today, due!);
  if (delta < 0) return 'overdue';
  if (delta === 0) return 'today';
  if (delta === 1) return 'tomorrow';
  if (delta <= 7) return 'week';
  return 'later';
}

/** Explicit status when present; legacy items only carry the `done` flag. */
export function taskStatus(item: TodoItem): TaskStatus {
  if (item.status) return item.status;
  return item.done ? 'done' : 'todo';
}

/** Kanban columns: fixed order, every task in exactly one column. */
export function groupByStatus(tasks: PlannedTask[]): Record<TaskStatus, PlannedTask[]> {
  const columns: Record<TaskStatus, PlannedTask[]> = { todo: [], doing: [], done: [] };
  for (const task of tasks) {
    columns[taskStatus(task.item)].push(task);
  }
  for (const id of KANBAN_ORDER) columns[id] = sortTasks(columns[id]);
  return columns;
}

export const KANBAN_ORDER: TaskStatus[] = ['todo', 'doing', 'done'];

/** Stable identity of a task across the vault: `blockId:itemId`. */
export function taskKey(task: PlannedTask): string {
  return `${task.blockId}:${task.item.id}`;
}

/** First day of a task: explicit start, otherwise the due day. */
export function taskStart(item: TodoItem): string | undefined {
  if (isDateKey(item.start)) return item.start;
  if (isDateKey(item.due)) return item.due;
  return undefined;
}

export interface GanttRange {
  start: string;
  end: string;
}

/** Timeline bounds: one day of padding around every dated task. */
export function ganttRange(tasks: PlannedTask[], today: string): GanttRange {
  let min: string | null = null;
  let max: string | null = null;
  for (const task of tasks) {
    const start = taskStart(task.item);
    const end = isDateKey(task.item.due) ? task.item.due! : start;
    if (!start || !end) continue;
    const lo = start <= end ? start : end;
    const hi = start <= end ? end : start;
    if (!min || lo < min) min = lo;
    if (!max || hi > max) max = hi;
  }
  if (!min || !max) {
    return { start: addDays(today, -1), end: addDays(today, 13) };
  }
  return { start: addDays(min, -1), end: addDays(max, 1) };
}

export interface GanttBar {
  left: number;
  span: number;
}

/** Bar geometry in whole days, relative to `range.start`. */
export function taskBar(task: PlannedTask, range: GanttRange): GanttBar | null {
  const start = taskStart(task.item);
  if (!start) return null;
  const due = isDateKey(task.item.due) ? task.item.due! : start;
  const lo = start <= due ? start : due;
  const hi = start <= due ? due : start;
  return {
    left: daysBetween(range.start, lo),
    span: Math.max(1, daysBetween(lo, hi) + 1),
  };
}

/**
 * Adding `dependencyKey` as a blocker of `targetKey` is a cycle when the
 * dependency already (transitively) depends on the target.
 */
export function wouldCreateCycle(
  tasks: PlannedTask[],
  targetKey: string,
  dependencyKey: string,
): boolean {
  if (targetKey === dependencyKey) return true;
  const byKey = new Map(tasks.map((task) => [taskKey(task), task]));
  const seen = new Set<string>();
  const stack = [dependencyKey];
  while (stack.length > 0) {
    const current = stack.pop()!;
    if (current === targetKey) return true;
    if (seen.has(current)) continue;
    seen.add(current);
    const task = byKey.get(current);
    for (const blocker of task?.item.blockedBy || []) stack.push(blocker);
  }
  return false;
}

/** A task is blocked while at least one prerequisite is not done. */
export function isBlocked(task: PlannedTask, tasks: PlannedTask[]): boolean {
  if (taskStatus(task.item) === 'done') return false;
  const byKey = new Map(tasks.map((candidate) => [taskKey(candidate), candidate]));
  return (task.item.blockedBy || []).some((key) => {
    const blocker = byKey.get(key);
    return blocker ? taskStatus(blocker.item) !== 'done' : false;
  });
}

function compareTasks(a: PlannedTask, b: PlannedTask): number {
  const dueA = isDateKey(a.item.due) ? a.item.due! : '9999-12-31';
  const dueB = isDateKey(b.item.due) ? b.item.due! : '9999-12-31';
  if (dueA !== dueB) return dueA < dueB ? -1 : 1;
  const pa = PRIORITY_WEIGHT[(a.item.priority || 'normal') as TaskPriority] ?? 1;
  const pb = PRIORITY_WEIGHT[(b.item.priority || 'normal') as TaskPriority] ?? 1;
  if (pa !== pb) return pa - pb;
  return a.item.text.localeCompare(b.item.text, 'ru');
}

export function sortTasks(tasks: PlannedTask[]): PlannedTask[] {
  return [...tasks].sort(compareTasks);
}

/** Group tasks into the fixed agenda buckets, skipping empty sections. */
export function groupTasks(
  tasks: PlannedTask[],
  today: string,
  options: { titles?: Record<BucketId, string> } = {},
): TaskSection[] {
  const titles = options.titles || BUCKET_TITLES;
  const order: BucketId[] = ['overdue', 'today', 'tomorrow', 'week', 'later', 'none', 'done'];
  const buckets = new Map<BucketId, PlannedTask[]>(order.map((id) => [id, []]));
  for (const task of tasks) {
    buckets.get(bucketOf(task, today))!.push(task);
  }
  return order
    .map((id) => ({ id, title: titles[id], tasks: sortTasks(buckets.get(id)!) }))
    .filter((section) => section.tasks.length > 0);
}

export function taskStats(tasks: PlannedTask[], today: string): TaskStats {
  let done = 0;
  let overdue = 0;
  let dueToday = 0;
  for (const task of tasks) {
    if (taskStatus(task.item) === 'done') {
      done += 1;
      continue;
    }
    const bucket = bucketOf(task, today);
    if (bucket === 'overdue') overdue += 1;
    if (bucket === 'today') dueToday += 1;
  }
  const total = tasks.length;
  return {
    total,
    done,
    open: total - done,
    overdue,
    today: dueToday,
    completionRate: total === 0 ? 0 : Math.round((done / total) * 100),
  };
}

/** Tasks due on one specific calendar day (open only, unless asked otherwise). */
export function tasksOnDay(tasks: PlannedTask[], day: string, includeDone = false): PlannedTask[] {
  return sortTasks(
    tasks.filter(
      (t) => t.item.due === day && (includeDone || taskStatus(t.item) !== 'done'),
    ),
  );
}

// ── ICS (RFC 5545) export ─────────────────────────────────────────────────────

const ICS_UID_DOMAIN = 'capsuleos.local';

/** RFC 5545 §3.3.11 TEXT escaping: backslash, semicolon, comma, newline. */
export function escapeIcsText(value: string): string {
  return value
    .replace(/\\/g, '\\\\')
    .replace(/;/g, '\\;')
    .replace(/,/g, '\\,')
    .replace(/\r\n|\r|\n/g, '\\n');
}

const encoder = new TextEncoder();

/**
 * RFC 5545 §3.1 line folding: no line may exceed 75 octets (excluding CRLF),
 * continuation lines start with a single space. Multi-byte characters are
 * never split across folds.
 */
export function foldIcsLine(line: string): string[] {
  if (encoder.encode(line).length <= 75) return [line];
  const chunks: string[] = [];
  let current = '';
  let bytes = 0;
  let limit = 75;
  for (const char of line) {
    const size = encoder.encode(char).length;
    if (bytes + size > limit) {
      chunks.push(current);
      current = char;
      bytes = size;
      limit = 74; // continuation lines: 1 octet is consumed by the leading space
    } else {
      current += char;
      bytes += size;
    }
  }
  if (current) chunks.push(current);
  return chunks.map((chunk, index) => (index === 0 ? chunk : ` ${chunk}`));
}

const formatUtcStamp = (date: Date): string =>
  `${date.getUTCFullYear()}${pad2(date.getUTCMonth() + 1)}${pad2(date.getUTCDate())}T` +
  `${pad2(date.getUTCHours())}${pad2(date.getUTCMinutes())}${pad2(date.getUTCSeconds())}Z`;

const sanitizeUid = (value: string): string => value.replace(/[^A-Za-z0-9._-]+/g, '-');

export interface IcsOptions {
  /** Injectable clock for deterministic tests. */
  now?: Date;
  calendarName?: string;
  /** Include already completed tasks (STATUS:COMPLETED). Default: false. */
  includeCompleted?: boolean;
  /** Output language of DESCRIPTION strings. Default: 'en'. */
  lang?: 'ru' | 'en';
}

const ICS_LABELS = {
  en: { untitled: '(untitled)', capsule: 'Capsule: {subject}', list: 'List: {title}', tags: 'Tags: {tags}' },
  ru: { untitled: '(без названия)', capsule: 'Капсула: {subject}', list: 'Список: {title}', tags: 'Теги: {tags}' },
} as const;

function icsPriority(priority: TaskPriority | undefined): number {
  switch (priority) {
    case 'high':
      return 1;
    case 'low':
      return 9;
    default:
      return 5;
  }
}

/** Build a VCALENDAR with one VTODO per task. */
export function buildIcs(tasks: PlannedTask[], options: IcsOptions = {}): string {
  const now = options.now || new Date();
  const stamp = formatUtcStamp(now);
  const calendarName = options.calendarName || 'CapsuleCraft Planning';
  const lang = options.lang || 'en';
  const labels = ICS_LABELS[lang];
  const selected = options.includeCompleted
    ? tasks
    : tasks.filter((t) => taskStatus(t.item) !== 'done');

  const lines: string[] = [
    'BEGIN:VCALENDAR',
    'VERSION:2.0',
    `PRODID:-//CapsuleCraft//Planning//${lang.toUpperCase()}`,
    'CALSCALE:GREGORIAN',
    'METHOD:PUBLISH',
    `X-WR-CALNAME:${escapeIcsText(calendarName)}`,
  ];

  for (const task of selected) {
    const uid = `${sanitizeUid(task.blockId)}.${sanitizeUid(task.item.id)}@${ICS_UID_DOMAIN}`;
    const summary = task.item.text.trim() || labels.untitled;
    const descriptionParts = [labels.capsule.replace('{subject}', task.capsuleSubject)];
    if (task.blockTitle) descriptionParts.push(labels.list.replace('{title}', task.blockTitle));
    if (task.capsuleTags.length) {
      descriptionParts.push(labels.tags.replace('{tags}', task.capsuleTags.join(' ')));
    }

    lines.push('BEGIN:VTODO');
    lines.push(`UID:${uid}`);
    lines.push(`DTSTAMP:${stamp}`);
    if (isDateKey(task.item.due)) {
      lines.push(`DUE;VALUE=DATE:${task.item.due!.replace(/-/g, '')}`);
    }
    lines.push(`SUMMARY:${escapeIcsText(summary)}`);
    lines.push(`DESCRIPTION:${escapeIcsText(descriptionParts.join('\n'))}`);
    const status = taskStatus(task.item);
    lines.push(`STATUS:${status === 'done' ? 'COMPLETED' : status === 'doing' ? 'IN-PROCESS' : 'NEEDS-ACTION'}`);
    lines.push(`PRIORITY:${icsPriority(task.item.priority as TaskPriority | undefined)}`);
    if (task.capsuleTags.length) {
      lines.push(`CATEGORIES:${task.capsuleTags.map((tag) => escapeIcsText(tag)).join(',')}`);
    }
    if (status === 'done') {
      lines.push('PERCENT-COMPLETE:100');
    }
    lines.push('END:VTODO');
  }

  lines.push('END:VCALENDAR');
  return lines.flatMap(foldIcsLine).join('\r\n') + '\r\n';
}

export function icsFileName(now = new Date()): string {
  return `capsuleos-planning-${toDateKey(now)}.ics`;
}
