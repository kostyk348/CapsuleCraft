import React, { useMemo, useState } from 'react';
import {
  AlertTriangle,
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
  Circle,
  Download,
  ExternalLink,
  Flag,
  Filter,
  Link2,
  ListTodo,
  Lock,
  Plus,
  RotateCcw,
  Search,
  Sun,
  X,
} from 'lucide-react';
import { CapsuleNode, TaskStatus, TodoItem, TodoPriority } from '../types';
import {
  bucketOf,
  buildIcs,
  addDays,
  collectTasks,
  daysBetween,
  EMPTY_TASK_FILTERS,
  filterTasks,
  ganttRange,
  groupByStatus,
  groupTasks,
  hasActiveTaskFilters,
  humanDate,
  icsFileName,
  isBlocked,
  KANBAN_ORDER,
  monthGrid,
  monthLabel,
  parseDateKey,
  PlannedTask,
  taskBar,
  taskKey,
  taskStart,
  taskStats,
  taskStatus,
  taskTags,
  tasksOnDay,
  toDateKey,
  wouldCreateCycle,
  type BucketId,
  type TaskDueFilter,
  type TaskFilterStatus,
  type TaskFilters,
} from '../engine/planning';
import { useI18n, type TranslationKey } from '../i18n';

interface Props {
  nodes: CapsuleNode[];
  onOpenCapsule: (messageId: string) => void;
  onToggleTask: (capsuleId: string, blockId: string, itemId: string) => void;
  onUpdateTask: (capsuleId: string, blockId: string, itemId: string, patch: Partial<TodoItem>) => void;
  /** Adds the task to the vault's inbox capsule (created on demand). */
  onAddTask: (text: string, due?: string, priority?: TodoPriority) => void;
}

const WEEKDAY_KEYS: TranslationKey[] = [
  'planning.weekday.mon',
  'planning.weekday.tue',
  'planning.weekday.wed',
  'planning.weekday.thu',
  'planning.weekday.fri',
  'planning.weekday.sat',
  'planning.weekday.sun',
];

const PRIORITY_KEY: Record<TodoPriority, TranslationKey> = {
  high: 'priority.high',
  normal: 'priority.normal',
  low: 'priority.low',
};

const BUCKET_KEY: Record<BucketId, TranslationKey> = {
  overdue: 'bucket.overdue',
  today: 'bucket.today',
  tomorrow: 'bucket.tomorrow',
  week: 'bucket.week',
  later: 'bucket.later',
  none: 'bucket.none',
  done: 'bucket.done',
};

const PRIORITY_CLASS: Record<TodoPriority, string> = {
  high: 'text-rose-300 bg-rose-950/60 border-rose-800/60',
  normal: 'text-slate-400 bg-slate-900 border-slate-700',
  low: 'text-slate-500 bg-slate-900 border-slate-800',
};

const STATUS_KEY: Record<TaskStatus, TranslationKey> = {
  todo: 'status.todo',
  doing: 'status.doing',
  done: 'status.done',
};

const FILTER_STATUSES: TaskFilterStatus[] = ['all', 'open', 'todo', 'doing', 'done'];
const FILTER_DUES: TaskDueFilter[] = ['all', 'overdue', 'today', 'week', 'none'];

const FILTER_STATUS_KEY: Record<TaskFilterStatus, TranslationKey> = {
  all: 'planning.filter.anyStatus',
  open: 'planning.filter.open',
  todo: 'status.todo',
  doing: 'status.doing',
  done: 'status.done',
};

const FILTER_DUE_KEY: Record<TaskDueFilter, TranslationKey> = {
  all: 'planning.filter.anyDue',
  overdue: 'planning.filter.overdue',
  today: 'planning.filter.today',
  week: 'planning.filter.week',
  none: 'planning.filter.none',
};

const STATUS_ACCENT: Record<TaskStatus, string> = {
  todo: 'text-slate-300 border-slate-700',
  doing: 'text-cyan-300 border-cyan-800/60',
  done: 'text-emerald-300 border-emerald-800/60',
};

const STATUS_DOT: Record<TaskStatus, string> = {
  todo: 'bg-slate-400',
  doing: 'bg-cyan-400 animate-pulse',
  done: 'bg-emerald-400',
};

const GANTT_DAY_W = 34;
const GANTT_ROW_H = 42;
const GANTT_LABEL_W = 240;

const BUCKET_ACCENT: Record<string, string> = {
  overdue: 'text-rose-400',
  today: 'text-cyan-300',
  tomorrow: 'text-sky-300',
  week: 'text-indigo-300',
  later: 'text-slate-400',
  none: 'text-slate-500',
  done: 'text-emerald-400',
};

function downloadText(fileName: string, text: string, mime: string) {
  const blob = new Blob([text], { type: mime });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

export const PlanningView: React.FC<Props> = ({
  nodes,
  onOpenCapsule,
  onToggleTask,
  onUpdateTask,
  onAddTask,
}) => {
  const { t, locale } = useI18n();
  const weekdays = useMemo(() => WEEKDAY_KEYS.map((k) => t(k)), [t]);
  const dateLocale = locale === 'en' ? 'en-GB' : 'ru-RU';
  const today = useMemo(() => toDateKey(new Date()), []);
  const [cursor, setCursor] = useState(() => {
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const [focusDay, setFocusDay] = useState<string | null>(null);
  const [showDone, setShowDone] = useState(false);
  const [viewMode, setViewMode] = useState<'agenda' | 'kanban' | 'gantt'>('agenda');
  const [filters, setFilters] = useState<TaskFilters>(EMPTY_TASK_FILTERS);
  const [draggingTask, setDraggingTask] = useState<string | null>(null);
  const [quickText, setQuickText] = useState('');
  const [quickDue, setQuickDue] = useState(today);
  const [quickPriority, setQuickPriority] = useState<TodoPriority>('normal');

  const allTasks = useMemo(() => collectTasks(nodes), [nodes]);
  const tasks = useMemo(
    () => filterTasks(allTasks, filters, today),
    [allTasks, filters, today],
  );
  const filtersActive = hasActiveTaskFilters(filters);
  // `showDone` is the quick toggle; an explicit `done` status filter wins.
  const displayTasks = useMemo(
    () =>
      showDone || filters.status === 'done'
        ? tasks
        : tasks.filter((task) => taskStatus(task.item) !== 'done'),
    [tasks, showDone, filters.status],
  );
  const stats = useMemo(() => taskStats(tasks, today), [tasks, today]);
  const capsuleOptions = useMemo(() => {
    const byId = new Map<string, string>();
    for (const task of allTasks) byId.set(task.capsuleId, task.capsuleSubject);
    return Array.from(byId, ([id, subject]) => ({ id, subject })).sort((a, b) =>
      (a.subject || '').localeCompare(b.subject || ''),
    );
  }, [allTasks]);
  const tagOptions = useMemo(() => taskTags(allTasks), [allTasks]);
  const cells = useMemo(() => monthGrid(cursor.year, cursor.month), [cursor]);
  const currentMonth = useMemo(
    () => new Date(cursor.year, cursor.month, 1).getMonth(),
    [cursor],
  );

  /** Open tasks per day, for the calendar dots. */
  const countByDay = useMemo(() => {
    const map = new Map<string, number>();
    for (const task of displayTasks) {
      if (taskStatus(task.item) === 'done' || !task.item.due) continue;
      map.set(task.item.due, (map.get(task.item.due) || 0) + 1);
    }
    return map;
  }, [displayTasks]);

  const sections = useMemo(
    () =>
      groupTasks(displayTasks, today, {
        titles: {
          overdue: t('bucket.overdue'),
          today: t('bucket.today'),
          tomorrow: t('bucket.tomorrow'),
          week: t('bucket.week'),
          later: t('bucket.later'),
          none: t('bucket.none'),
          done: t('bucket.done'),
        },
      }),
    [displayTasks, today, t],
  );

  const board = useMemo(() => groupByStatus(displayTasks), [displayTasks]);

  const ganttTasks = useMemo(
    () =>
      displayTasks
        .filter((task) => taskStart(task.item))
        .sort((a, b) => {
          const startA = taskStart(a.item) || '';
          const startB = taskStart(b.item) || '';
          if (startA !== startB) return startA < startB ? -1 : 1;
          return (a.item.due || '').localeCompare(b.item.due || '');
        }),
    [displayTasks],
  );
  const undatedTasks = useMemo(
    () => displayTasks.filter((task) => !taskStart(task.item)),
    [displayTasks],
  );
  const range = useMemo(() => ganttRange(ganttTasks, today), [ganttTasks, today]);
  const totalDays = useMemo(() => daysBetween(range.start, range.end) + 1, [range]);
  const barsByKey = useMemo(() => {
    const map = new Map<string, { left: number; span: number }>();
    for (const task of ganttTasks) {
      const bar = taskBar(task, range);
      if (bar) map.set(taskKey(task), bar);
    }
    return map;
  }, [ganttTasks, range]);
  const todayOffset = useMemo(() => daysBetween(range.start, today), [range.start, today]);

  const focusTasks = focusDay ? tasksOnDay(displayTasks, focusDay, true) : [];

  const shiftMonth = (delta: number) => {
    setCursor((prev) => {
      const date = new Date(prev.year, prev.month + delta, 1);
      return { year: date.getFullYear(), month: date.getMonth() };
    });
  };

  const goToday = () => {
    const now = new Date();
    setCursor({ year: now.getFullYear(), month: now.getMonth() });
    setFocusDay(today);
  };

  const submitQuickAdd = (event?: React.FormEvent) => {
    event?.preventDefault();
    const text = quickText.trim();
    if (!text) return;
    onAddTask(text, quickDue || undefined, quickPriority);
    setQuickText('');
    // A new task that the current filter hides would look like a lost write.
    if (filtersActive) setFilters(EMPTY_TASK_FILTERS);
  };

  const clearFilters = () => setFilters(EMPTY_TASK_FILTERS);

  const exportIcs = () => {
    // Export what the user currently sees; displayTasks already applied the
    // completed-task visibility rule, so allow completed rows through.
    const ics = buildIcs(displayTasks, {
      includeCompleted: true,
      calendarName: 'CapsuleCraft Planning',
      lang: locale,
    });
    downloadText(icsFileName(), ics, 'text/calendar;charset=utf-8');
  };

  const renderTask = (task: PlannedTask) => {
    const overdue = taskStatus(task.item) !== 'done' && bucketOf(task, today) === 'overdue';
    return (
      <div
        key={`${task.blockId}:${task.item.id}`}
        className="group flex items-start gap-2.5 px-3 py-2 rounded-lg border border-transparent hover:border-slate-700/70 hover:bg-slate-900/60 transition-colors"
      >
        <button
          type="button"
          onClick={() => onToggleTask(task.capsuleId, task.blockId, task.item.id)}
          className="mt-0.5 shrink-0 text-slate-500 hover:text-cyan-300 transition-colors"
          title={task.item.done ? t('planning.markUndone') : t('planning.markDone')}
        >
          {task.item.done ? (
            <Check className="w-4 h-4 text-emerald-400" />
          ) : (
            <Circle className="w-4 h-4" />
          )}
        </button>

        <div className="min-w-0 flex-1">
          <div
            className={`text-sm leading-snug break-words ${
              task.item.done ? 'text-slate-500 line-through' : 'text-slate-200'
            }`}
          >
            {task.item.text}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-1.5 text-[10px] font-mono">
            <button
              type="button"
              onClick={() => onOpenCapsule(task.capsuleId)}
              className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-slate-900 border border-slate-800 text-slate-400 hover:text-cyan-300 hover:border-cyan-800/60 max-w-[240px]"
              title={t('planning.openCapsule', { subject: task.capsuleSubject })}
            >
              <ExternalLink className="w-2.5 h-2.5 shrink-0" />
              <span className="truncate">{task.capsuleSubject}</span>
            </button>
            {task.item.due && (
              <span
                className={`px-1.5 py-0.5 rounded border ${
                  overdue
                    ? 'text-rose-300 bg-rose-950/50 border-rose-900/60'
                    : 'text-slate-400 bg-slate-900 border-slate-800'
                }`}
              >
                {humanDate(task.item.due, dateLocale)}
              </span>
            )}
            <span
              className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded border ${
                PRIORITY_CLASS[(task.item.priority || 'normal') as TodoPriority]
              }`}
              title={`${t('planning.priorityTitle')}: ${t(PRIORITY_KEY[(task.item.priority || 'normal') as TodoPriority])}`}
            >
              <Flag className="w-2.5 h-2.5" />
              {task.item.priority === 'high' ? 'high' : task.item.priority === 'low' ? 'low' : 'norm'}
            </span>
          </div>
        </div>

        <input
          type="date"
          value={task.item.due || ''}
          onChange={(event) =>
            onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
              due: event.target.value || undefined,
            })
          }
          className="shrink-0 w-[118px] text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1.5 py-1 text-slate-400 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity [color-scheme:dark]"
          title={t('planning.dueTitle')}
        />

        <select
          value={task.item.priority || 'normal'}
          onChange={(event) =>
            onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
              priority: event.target.value as TodoPriority,
            })
          }
          className="shrink-0 text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1 py-1 text-slate-400 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
          title={t('planning.priorityTitle')}
        >
          {(Object.keys(PRIORITY_KEY) as TodoPriority[]).map((value) => (
            <option key={value} value={value}>
              {t(PRIORITY_KEY[value])}
            </option>
          ))}
        </select>
      </div>
    );
  };

  return (
    <div className="w-full h-full overflow-y-auto bg-slate-950">
      <div className="max-w-7xl mx-auto p-4 md:p-6 space-y-4">
        {/* Header */}
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-lg font-bold text-slate-100 flex items-center gap-2">
              <CalendarDays className="w-5 h-5 text-cyan-400" />
              {t('planning.title')}
            </h2>
            <p className="text-[11px] text-slate-500 font-mono mt-0.5">
              {t('planning.subtitle')}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex rounded-lg border border-slate-800 overflow-hidden">
              <button
                type="button"
                onClick={() => setViewMode('agenda')}
                className={`text-[11px] font-mono px-2.5 py-1.5 transition-colors ${
                  viewMode === 'agenda'
                    ? 'bg-cyan-500/15 text-cyan-300'
                    : 'bg-slate-900 text-slate-400 hover:text-slate-200'
                }`}
              >
                {t('planning.viewAgenda')}
              </button>
              <button
                type="button"
                onClick={() => setViewMode('kanban')}
                className={`text-[11px] font-mono px-2.5 py-1.5 border-l border-slate-800 transition-colors ${
                  viewMode === 'kanban'
                    ? 'bg-cyan-500/15 text-cyan-300'
                    : 'bg-slate-900 text-slate-400 hover:text-slate-200'
                }`}
              >
                {t('planning.viewKanban')}
              </button>
              <button
                type="button"
                onClick={() => setViewMode('gantt')}
                className={`text-[11px] font-mono px-2.5 py-1.5 border-l border-slate-800 transition-colors ${
                  viewMode === 'gantt'
                    ? 'bg-cyan-500/15 text-cyan-300'
                    : 'bg-slate-900 text-slate-400 hover:text-slate-200'
                }`}
              >
                {t('planning.viewGantt')}
              </button>
            </div>
            <button
              type="button"
              onClick={() => setShowDone((v) => !v)}
              className={`text-[11px] font-mono px-2.5 py-1.5 rounded-lg border transition-colors ${
                showDone
                  ? 'bg-emerald-500/15 text-emerald-300 border-emerald-700/60'
                  : 'bg-slate-900 text-slate-400 border-slate-800 hover:text-slate-200'
              }`}
            >
              {showDone ? t('planning.hideDone') : t('planning.showDone')}
            </button>
            <button
              type="button"
              onClick={exportIcs}
              disabled={displayTasks.length === 0}
              className="inline-flex items-center gap-1.5 text-[11px] font-mono px-2.5 py-1.5 rounded-lg border border-slate-700 bg-slate-900 text-slate-300 hover:text-cyan-300 hover:border-cyan-700/60 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
              title={t('planning.exportTitle')}
            >
              <Download className="w-3.5 h-3.5" />
              {t('planning.export')}
            </button>
          </div>
        </div>

        {/* Cross-project task filters */}
        <div
          className="rounded-xl border border-slate-800 bg-slate-900/40 p-2.5 flex flex-wrap items-center gap-2"
          data-testid="task-filters"
        >
          <div className="flex items-center gap-2 px-1 text-cyan-400 shrink-0">
            <Filter className="w-3.5 h-3.5" />
            <span className="text-[10px] font-mono uppercase tracking-wider text-slate-500">
              {t('planning.filter.title')}
            </span>
          </div>

          <div className="relative flex-1 min-w-[180px]">
            <Search className="w-3.5 h-3.5 absolute left-2.5 top-2 text-slate-500" />
            <input
              type="text"
              value={filters.text || ''}
              onChange={(event) =>
                setFilters((prev) => ({ ...prev, text: event.target.value }))
              }
              placeholder={t('planning.filter.text')}
              className="w-full bg-slate-950/70 border border-slate-800 rounded-md pl-8 pr-7 py-1.5 text-[11px] font-mono text-slate-200 placeholder:text-slate-600 outline-none focus:border-cyan-500/60"
            />
            {!!filters.text && (
              <button
                type="button"
                onClick={() => setFilters((prev) => ({ ...prev, text: '' }))}
                className="absolute right-1.5 top-1.5 p-0.5 text-slate-500 hover:text-slate-200"
                title={t('planning.filter.clearText')}
              >
                <X className="w-3 h-3" />
              </button>
            )}
          </div>

          <select
            value={filters.status || 'all'}
            onChange={(event) =>
              setFilters((prev) => ({
                ...prev,
                status: event.target.value as TaskFilterStatus,
              }))
            }
            className="text-[11px] font-mono bg-slate-950/70 border border-slate-800 rounded-md px-2 py-1.5 text-slate-300"
            title={t('planning.filter.statusTitle')}
          >
            {FILTER_STATUSES.map((status) => (
              <option key={status} value={status}>
                {t(FILTER_STATUS_KEY[status])}
              </option>
            ))}
          </select>

          <select
            value={filters.priority || 'all'}
            onChange={(event) =>
              setFilters((prev) => ({
                ...prev,
                priority: event.target.value as TodoPriority | 'all',
              }))
            }
            className="text-[11px] font-mono bg-slate-950/70 border border-slate-800 rounded-md px-2 py-1.5 text-slate-300"
            title={t('planning.priorityTitle')}
          >
            <option value="all">{t('planning.filter.anyPriority')}</option>
            {(Object.keys(PRIORITY_KEY) as TodoPriority[]).map((value) => (
              <option key={value} value={value}>
                {t(PRIORITY_KEY[value])}
              </option>
            ))}
          </select>

          <select
            value={filters.due || 'all'}
            onChange={(event) =>
              setFilters((prev) => ({
                ...prev,
                due: event.target.value as TaskDueFilter,
              }))
            }
            className="text-[11px] font-mono bg-slate-950/70 border border-slate-800 rounded-md px-2 py-1.5 text-slate-300"
            title={t('planning.dueTitle')}
          >
            {FILTER_DUES.map((due) => (
              <option key={due} value={due}>
                {t(FILTER_DUE_KEY[due])}
              </option>
            ))}
          </select>

          <select
            value={filters.capsuleId || ''}
            onChange={(event) =>
              setFilters((prev) => ({ ...prev, capsuleId: event.target.value || null }))
            }
            className="text-[11px] font-mono bg-slate-950/70 border border-slate-800 rounded-md px-2 py-1.5 text-slate-300 max-w-[190px]"
            title={t('planning.filter.capsuleTitle')}
          >
            <option value="">{t('planning.filter.allCapsules')}</option>
            {capsuleOptions.map((capsule) => (
              <option key={capsule.id} value={capsule.id}>
                {capsule.subject || t('editor.untitled')}
              </option>
            ))}
          </select>

          {tagOptions.length > 0 && (
            <select
              value={filters.tag || ''}
              onChange={(event) =>
                setFilters((prev) => ({ ...prev, tag: event.target.value || null }))
              }
              className="text-[11px] font-mono bg-slate-950/70 border border-slate-800 rounded-md px-2 py-1.5 text-slate-300"
              title={t('planning.filter.tagTitle')}
            >
              <option value="">{t('planning.filter.allTags')}</option>
              {tagOptions.map((tag) => (
                <option key={tag} value={tag}>
                  {tag}
                </option>
              ))}
            </select>
          )}

          {filtersActive && (
            <button
              type="button"
              onClick={clearFilters}
              className="inline-flex items-center gap-1 text-[10px] font-mono px-2 py-1.5 rounded-md border border-rose-800/60 text-rose-300 bg-rose-950/30 hover:bg-rose-900/40 transition-colors"
              title={t('planning.filter.clear')}
            >
              <X className="w-3 h-3" />
              {t('planning.filter.clearShort')}
            </button>
          )}

          <span className="ml-auto text-[10px] font-mono text-slate-500 tabular-nums">
            {t('planning.filter.count', {
              shown: displayTasks.length,
              total: allTasks.length,
            })}
          </span>
        </div>

        {/* Stats */}
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-2.5">
          {[
            { label: t('planning.stat.open'), value: stats.open, icon: ListTodo, tone: 'text-slate-200' },
            { label: t('planning.stat.overdue'), value: stats.overdue, icon: AlertTriangle, tone: stats.overdue ? 'text-rose-400' : 'text-slate-200' },
            { label: t('planning.stat.today'), value: stats.today, icon: Sun, tone: stats.today ? 'text-cyan-300' : 'text-slate-200' },
            { label: t('planning.stat.done'), value: `${stats.completionRate}%`, icon: Check, tone: 'text-emerald-400' },
          ].map(({ label, value, icon: Icon, tone }) => (
            <div
              key={label}
              className="rounded-xl border border-slate-800 bg-slate-900/60 px-3 py-2.5 flex items-center gap-3"
            >
              <Icon className={`w-4 h-4 ${tone}`} />
              <div>
                <div className={`text-lg font-bold leading-none font-mono ${tone}`}>{value}</div>
                <div className="text-[10px] text-slate-500 font-mono mt-1">{label}</div>
              </div>
            </div>
          ))}
        </div>

        {viewMode === 'agenda' ? (
        <div className="grid grid-cols-1 lg:grid-cols-[minmax(280px,340px)_1fr] gap-4 items-start">
          {/* Month calendar */}
          <div className="rounded-xl border border-slate-800 bg-slate-900/40 p-3">
            <div className="flex items-center justify-between mb-2">
              <button
                type="button"
                onClick={() => shiftMonth(-1)}
                className="p-1.5 rounded-md text-slate-400 hover:text-cyan-300 hover:bg-slate-800"
                title={t('planning.prevMonth')}
              >
                <ChevronLeft className="w-4 h-4" />
              </button>
              <div className="flex items-center gap-2">
                <span className="text-sm font-semibold text-slate-200 capitalize">
                  {monthLabel(cursor.year, cursor.month, dateLocale)}
                </span>
                <button
                  type="button"
                  onClick={goToday}
                  className="text-[10px] font-mono px-1.5 py-0.5 rounded border border-slate-700 text-slate-400 hover:text-cyan-300 hover:border-cyan-700/60"
                  title={t('planning.goToday')}
                >
                  <RotateCcw className="w-3 h-3" />
                </button>
              </div>
              <button
                type="button"
                onClick={() => shiftMonth(1)}
                className="p-1.5 rounded-md text-slate-400 hover:text-cyan-300 hover:bg-slate-800"
                title={t('planning.nextMonth')}
              >
                <ChevronRight className="w-4 h-4" />
              </button>
            </div>

            <div className="grid grid-cols-7 gap-1 text-center">
              {weekdays.map((day) => (
                <div key={day} className="text-[10px] font-mono text-slate-600 pb-1">
                  {day}
                </div>
              ))}
              {cells.map((key) => {
                const date = Number(key.slice(8, 10));
                const inMonth = parseDateKey(key)?.getMonth() === currentMonth;
                const count = countByDay.get(key) || 0;
                const isToday = key === today;
                const selected = key === focusDay;
                return (
                  <button
                    key={key}
                    type="button"
                    onClick={() => setFocusDay((prev) => (prev === key ? null : key))}
                    className={`relative h-9 rounded-md text-[11px] font-mono border transition-colors ${
                      selected
                        ? 'bg-cyan-500/20 border-cyan-500/50 text-cyan-200'
                        : isToday
                          ? 'border-cyan-700/60 text-cyan-300 hover:bg-slate-800'
                          : inMonth
                            ? 'border-transparent text-slate-300 hover:bg-slate-800'
                            : 'border-transparent text-slate-700 hover:bg-slate-800/50'
                    }`}
                    title={count ? t('planning.dayTitle', { day: key, count }) : key}
                  >
                    {date}
                    {count > 0 && (
                      <span className="absolute bottom-1 left-1/2 -translate-x-1/2 flex gap-0.5">
                        {Array.from({ length: Math.min(count, 3) }).map((_, index) => (
                          <span
                            key={index}
                            className={`w-1 h-1 rounded-full ${
                              key < today ? 'bg-rose-400' : key === today ? 'bg-cyan-300' : 'bg-slate-400'
                            }`}
                          />
                        ))}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
            <div className="mt-2 flex items-center gap-3 text-[10px] font-mono text-slate-600">
              <span className="flex items-center gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-cyan-300" /> {t('planning.legendToday')}
              </span>
              <span className="flex items-center gap-1">
                <span className="w-1.5 h-1.5 rounded-full bg-rose-400" /> {t('planning.legendOverdue')}
              </span>
            </div>
          </div>

          {/* Quick add + agenda */}
          <div className="rounded-xl border border-slate-800 bg-slate-900/40 p-3 min-h-[420px]">
            <form onSubmit={submitQuickAdd} className="flex flex-wrap items-center gap-2 pb-3 mb-3 border-b border-slate-800">
              <div className="flex items-center gap-2 flex-1 min-w-[200px]">
                <Plus className="w-4 h-4 text-cyan-400 shrink-0" />
                <input
                  value={quickText}
                  onChange={(event) => setQuickText(event.target.value)}
                  placeholder={t('planning.quickPlaceholder')}
                  className="w-full bg-transparent text-sm text-slate-200 placeholder:text-slate-600 outline-none"
                />
              </div>
              <input
                type="date"
                value={quickDue}
                onChange={(event) => setQuickDue(event.target.value)}
                className="text-[11px] font-mono bg-slate-900 border border-slate-800 rounded px-2 py-1 text-slate-300 [color-scheme:dark]"
                title={t('planning.dueTitle')}
              />
              <select
                value={quickPriority}
                onChange={(event) => setQuickPriority(event.target.value as TodoPriority)}
                className="text-[11px] font-mono bg-slate-900 border border-slate-800 rounded px-2 py-1 text-slate-300"
                title={t('planning.priorityTitle')}
              >
                {(Object.keys(PRIORITY_KEY) as TodoPriority[]).map((value) => (
                  <option key={value} value={value}>
                    {t(PRIORITY_KEY[value])}
                  </option>
                ))}
              </select>
              <button
                type="submit"
                disabled={!quickText.trim()}
                className="text-[11px] font-mono px-2.5 py-1 rounded bg-cyan-600 hover:bg-cyan-500 text-white disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
              >
                {t('planning.add')}
              </button>
            </form>

            {focusDay ? (
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <h3 className="text-xs font-semibold uppercase tracking-wider text-slate-300">
                    {humanDate(focusDay, dateLocale)}
                  </h3>
                  <button
                    type="button"
                    onClick={() => setFocusDay(null)}
                    className="text-[10px] font-mono text-slate-500 hover:text-cyan-300"
                  >
                    {t('planning.showAll')}
                  </button>
                </div>
                {focusTasks.length === 0 ? (
                  <p className="text-xs text-slate-600 font-mono py-6 text-center">
                    {t('planning.dayEmpty')}
                  </p>
                ) : (
                  <div className="space-y-0.5">{focusTasks.map(renderTask)}</div>
                )}
              </div>
            ) : sections.length === 0 ? (
              <div className="py-16 text-center space-y-2">
                <ListTodo className="w-8 h-8 text-slate-700 mx-auto" />
                <p className="text-sm text-slate-400">
                  {filtersActive ? t('planning.filter.noneMatch') : t('planning.emptyTitle')}
                </p>
                <p className="text-[11px] text-slate-600 font-mono max-w-sm mx-auto">
                  {filtersActive
                    ? t('planning.filter.noneMatchHint')
                    : t('planning.emptyHint', { command: '/todo' })}
                </p>
                {filtersActive && (
                  <button
                    type="button"
                    onClick={clearFilters}
                    className="text-[11px] font-mono px-2.5 py-1 rounded border border-slate-700 text-slate-300 hover:text-cyan-300 hover:border-cyan-700/60"
                  >
                    {t('planning.filter.clear')}
                  </button>
                )}
              </div>
            ) : (
              <div className="space-y-3">
                {sections.map((section) => (
                  <div key={section.id}>
                    <div className="flex items-center gap-2 mb-1 px-1">
                      <h3
                        className={`text-[11px] font-semibold uppercase tracking-wider ${
                          BUCKET_ACCENT[section.id] || 'text-slate-400'
                        }`}
                      >
                        {t(BUCKET_KEY[section.id])}
                      </h3>
                      <span className="text-[10px] font-mono text-slate-600">
                        {section.tasks.length}
                      </span>
                    </div>
                    <div className="space-y-0.5">{section.tasks.map(renderTask)}</div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
        ) : viewMode === 'kanban' ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3 items-start">
            {KANBAN_ORDER.map((status) => (
              <div
                key={status}
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  event.preventDefault();
                  const key = event.dataTransfer.getData('text/plain');
                  const task = displayTasks.find((candidate) => `${candidate.blockId}:${candidate.item.id}` === key);
                  setDraggingTask(null);
                  if (!task) return;
                  onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
                    status,
                    done: status === 'done',
                  });
                }}
                className="rounded-xl border border-slate-800 bg-slate-900/40 p-2 min-h-[360px]"
              >
                <div className="flex items-center gap-2 px-1.5 pb-2 mb-2 border-b border-slate-800">
                  <span className={`w-1.5 h-1.5 rounded-full ${STATUS_DOT[status]}`} />
                  <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-300">
                    {t(STATUS_KEY[status])}
                  </h3>
                  <span className="text-[10px] font-mono text-slate-600">{board[status].length}</span>
                </div>

                <div className="space-y-2">
                  {board[status].length === 0 ? (
                    <p className="text-[10px] font-mono text-slate-600 text-center py-6 px-2">
                      {t('planning.boardEmpty')}
                    </p>
                  ) : (
                    board[status].map((task) => {
                      const key = `${task.blockId}:${task.item.id}`;
                      const overdue = status !== 'done' && bucketOf(task, today) === 'overdue';
                      return (
                        <div
                          key={key}
                          draggable
                          onDragStart={(event) => {
                            event.dataTransfer.setData('text/plain', key);
                            event.dataTransfer.effectAllowed = 'move';
                            setDraggingTask(key);
                          }}
                          onDragEnd={() => setDraggingTask(null)}
                          className={`rounded-lg border bg-slate-950/70 p-2.5 cursor-grab active:cursor-grabbing transition-opacity ${
                            draggingTask === key ? 'opacity-40' : 'opacity-100'
                          } ${STATUS_ACCENT[status]}`}
                        >
                          <div className={`text-xs leading-snug break-words ${
                            status === 'done' ? 'text-slate-500 line-through' : 'text-slate-200'
                          }`}>
                            {task.item.text || t('todo.actionPlaceholder')}
                          </div>
                          <div className="mt-1.5 flex flex-wrap items-center gap-1 text-[9px] font-mono">
                            <button
                              type="button"
                              onClick={() => onOpenCapsule(task.capsuleId)}
                              className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded bg-slate-900 border border-slate-800 text-slate-400 hover:text-cyan-300 max-w-[180px]"
                              title={t('planning.openCapsule', { subject: task.capsuleSubject })}
                            >
                              <ExternalLink className="w-2.5 h-2.5 shrink-0" />
                              <span className="truncate">{task.capsuleSubject}</span>
                            </button>
                            {task.item.due && (
                              <span className={`px-1.5 py-0.5 rounded border ${
                                overdue
                                  ? 'text-rose-300 bg-rose-950/50 border-rose-900/60'
                                  : 'text-slate-400 bg-slate-900 border-slate-800'
                              }`}>
                                {humanDate(task.item.due, dateLocale)}
                              </span>
                            )}
                            <span className={`px-1.5 py-0.5 rounded border ${
                              PRIORITY_CLASS[(task.item.priority || 'normal') as TodoPriority]
                            }`}>
                              {task.item.priority === 'high' ? 'high' : task.item.priority === 'low' ? 'low' : 'norm'}
                            </span>
                          </div>
                          <select
                            value={taskStatus(task.item)}
                            onChange={(event) => {
                              const next = event.target.value as TaskStatus;
                              onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
                                status: next,
                                done: next === 'done',
                              });
                            }}
                            className="mt-1.5 w-full text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1.5 py-1 text-slate-400"
                            title={t('planning.statusTitle')}
                          >
                            {KANBAN_ORDER.map((option) => (
                              <option key={option} value={option}>
                                {t(STATUS_KEY[option])}
                              </option>
                            ))}
                          </select>
                        </div>
                      );
                    })
                  )}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="space-y-3">
            <div className="rounded-xl border border-slate-800 bg-slate-900/40 overflow-hidden">
              {ganttTasks.length === 0 ? (
                <p className="text-[11px] font-mono text-slate-500 text-center py-10 px-4">
                  {t('gantt.undated')}
                </p>
              ) : (
                <div className="overflow-x-auto">
                  <div style={{ minWidth: GANTT_LABEL_W + totalDays * GANTT_DAY_W }}>
                    {/* Timeline header */}
                    <div className="flex border-b border-slate-800 bg-slate-950/60">
                      <div
                        className="shrink-0 px-3 py-1.5 text-[10px] font-mono text-slate-500 sticky left-0 z-10 bg-slate-950/95 border-r border-slate-800/60"
                        style={{ width: GANTT_LABEL_W }}
                      >
                        {t('gantt.editor')}
                      </div>
                      <div className="relative h-6" style={{ width: totalDays * GANTT_DAY_W }}>
                        {Array.from({ length: Math.ceil(totalDays / 7) }).map((_, index) => (
                          <div
                            key={index}
                            className="absolute top-0 bottom-0 border-l border-slate-800/60 pl-1 text-[9px] font-mono text-slate-500 whitespace-nowrap"
                            style={{ left: index * 7 * GANTT_DAY_W }}
                          >
                            {humanDate(addDays(range.start, index * 7), dateLocale)}
                          </div>
                        ))}
                      </div>
                    </div>

                    {/* Rows + dependency arrows */}
                    <div className="relative">
                      <div
                        className="absolute inset-y-0 pointer-events-none"
                        style={{ left: GANTT_LABEL_W, width: totalDays * GANTT_DAY_W }}
                      >
                        {Array.from({ length: Math.ceil(totalDays / 7) }).map((_, index) => (
                          <div
                            key={index}
                            className="absolute top-0 bottom-0 border-l border-slate-800/40"
                            style={{ left: index * 7 * GANTT_DAY_W }}
                          />
                        ))}
                        {todayOffset >= 0 && todayOffset < totalDays && (
                          <div
                            className="absolute top-0 bottom-0 w-px bg-cyan-500/60"
                            style={{ left: todayOffset * GANTT_DAY_W }}
                            title={t('gantt.today')}
                          />
                        )}
                      </div>

                      {ganttTasks.map((task, index) => {
                        const key = taskKey(task);
                        const bar = barsByKey.get(key);
                        const blocked = isBlocked(task, allTasks);
                        const start = taskStart(task.item)!;
                        return (
                          <div
                            key={key}
                            className="flex border-b border-slate-800/50"
                            style={{ height: GANTT_ROW_H }}
                          >
                            <div
                              className="shrink-0 sticky left-0 z-10 bg-slate-950/95 px-3 flex flex-col justify-center border-r border-slate-800/60"
                              style={{ width: GANTT_LABEL_W }}
                            >
                              <div className="flex items-center gap-1.5 min-w-0">
                                {blocked && (
                                  <Lock
                                    className="w-3 h-3 text-amber-400 shrink-0"
                                    aria-label={t('gantt.blocked')}
                                  />
                                )}
                                <span
                                  className={`truncate text-[11px] ${
                                    taskStatus(task.item) === 'done'
                                      ? 'text-slate-500 line-through'
                                      : 'text-slate-200'
                                  }`}
                                >
                                  {task.item.text || t('todo.actionPlaceholder')}
                                </span>
                              </div>
                              <span className="text-[9px] font-mono text-slate-600 truncate">
                                {task.capsuleSubject}
                              </span>
                            </div>
                            <div className="relative" style={{ width: totalDays * GANTT_DAY_W }}>
                              {bar && (
                                <div
                                  className={`absolute top-1/2 -translate-y-1/2 h-5 rounded-md border text-[9px] font-mono flex items-center px-1.5 overflow-hidden ${
                                    blocked
                                      ? 'bg-amber-500/20 border-amber-500/60 text-amber-200'
                                      : taskStatus(task.item) === 'done'
                                        ? 'bg-emerald-500/15 border-emerald-700/60 text-emerald-300'
                                        : 'bg-cyan-500/20 border-cyan-500/60 text-cyan-200'
                                  }`}
                                  style={{
                                    left: bar.left * GANTT_DAY_W,
                                    width: Math.max(bar.span * GANTT_DAY_W, 16),
                                  }}
                                  title={`${humanDate(start, dateLocale)} → ${humanDate(task.item.due || start, dateLocale)}${blocked ? ` · ${t('gantt.blocked')}` : ''}`}
                                >
                                  <span className="truncate">{bar.span > 2 ? task.item.text : ''}</span>
                                </div>
                              )}
                            </div>
                          </div>
                        );
                      })}

                      <svg
                        className="absolute top-0 pointer-events-none"
                        style={{
                          left: GANTT_LABEL_W,
                          width: totalDays * GANTT_DAY_W,
                          height: ganttTasks.length * GANTT_ROW_H,
                        }}
                      >
                        <defs>
                          <marker
                            id="gantt-arrow"
                            markerWidth="6"
                            markerHeight="6"
                            refX="5"
                            refY="3"
                            orient="auto"
                          >
                            <path d="M0,0 L6,3 L0,6 z" fill="#f59e0b" />
                          </marker>
                        </defs>
                        {ganttTasks.flatMap((task, index) => {
                          const bar = barsByKey.get(taskKey(task));
                          if (!bar) return [];
                          return (task.item.blockedBy || []).map((dependencyKey) => {
                            const dependencyBar = barsByKey.get(dependencyKey);
                            const dependencyIndex = ganttTasks.findIndex(
                              (candidate) => taskKey(candidate) === dependencyKey,
                            );
                            if (!dependencyBar || dependencyIndex < 0) return null;
                            const x1 = (dependencyBar.left + dependencyBar.span) * GANTT_DAY_W;
                            const y1 = dependencyIndex * GANTT_ROW_H + GANTT_ROW_H / 2;
                            const x2 = bar.left * GANTT_DAY_W - 2;
                            const y2 = index * GANTT_ROW_H + GANTT_ROW_H / 2;
                            const elbow = x1 + 12;
                            return (
                              <path
                                key={`${dependencyKey}->${taskKey(task)}`}
                                d={`M${x1} ${y1} H${elbow} V${y2} H${x2}`}
                                fill="none"
                                stroke="#f59e0b"
                                strokeWidth="1.2"
                                markerEnd="url(#gantt-arrow)"
                                opacity="0.8"
                              />
                            );
                          });
                        })}
                      </svg>
                    </div>
                  </div>
                </div>
              )}
            </div>

            {undatedTasks.length > 0 && ganttTasks.length > 0 && (
              <p className="text-[10px] font-mono text-slate-500">{t('gantt.undated')}</p>
            )}

            {/* Dates and dependencies editor */}
            <div className="rounded-xl border border-slate-800 bg-slate-900/40 p-3">
              <div className="flex items-center gap-1.5 mb-2 text-[10px] font-mono uppercase tracking-wider text-slate-400">
                <Link2 className="w-3 h-3" />
                {t('gantt.editor')}
              </div>
              {displayTasks.length === 0 ? (
                <p className="text-[10px] font-mono text-slate-600 py-2">{t('planning.emptyTitle')}</p>
              ) : (
                <div className="space-y-1.5">
                  {displayTasks.map((task) => {
                    const key = taskKey(task);
                    const start = taskStart(task.item) || '';
                    const due = task.item.due || '';
                    const blocker = (task.item.blockedBy || [])[0] || '';
                    const candidates = allTasks.filter((candidate) => {
                      const candidateKey = taskKey(candidate);
                      return (
                        candidateKey !== key && !wouldCreateCycle(allTasks, key, candidateKey)
                      );
                    });
                    return (
                      <div
                        key={key}
                        className="grid grid-cols-1 md:grid-cols-[1fr_130px_130px_190px] items-center gap-1.5"
                      >
                        <span
                          className={`truncate text-[11px] ${
                            taskStatus(task.item) === 'done'
                              ? 'text-slate-500 line-through'
                              : 'text-slate-300'
                          }`}
                        >
                          {task.item.text || t('todo.actionPlaceholder')}
                        </span>
                        <input
                          type="date"
                          value={start}
                          onChange={(event) =>
                            onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
                              start: event.target.value || undefined,
                            })
                          }
                          className="text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1.5 py-1 text-slate-300 [color-scheme:dark]"
                          title={t('gantt.start')}
                        />
                        <input
                          type="date"
                          value={due}
                          onChange={(event) =>
                            onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
                              due: event.target.value || undefined,
                            })
                          }
                          className="text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1.5 py-1 text-slate-300 [color-scheme:dark]"
                          title={t('gantt.due')}
                        />
                        <select
                          value={blocker}
                          onChange={(event) =>
                            onUpdateTask(task.capsuleId, task.blockId, task.item.id, {
                              blockedBy: event.target.value ? [event.target.value] : [],
                            })
                          }
                          className="text-[10px] font-mono bg-slate-900 border border-slate-800 rounded px-1.5 py-1 text-slate-400 truncate"
                          title={t('gantt.dependsOn')}
                        >
                          <option value="">{t('gantt.none')}</option>
                          {candidates.map((candidate) => (
                            <option key={taskKey(candidate)} value={taskKey(candidate)}>
                              {candidate.item.text || t('todo.actionPlaceholder')}
                            </option>
                          ))}
                        </select>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
