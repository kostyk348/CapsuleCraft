import { describe, expect, it } from 'vitest';
import {
  addDays,
  buildIcs,
  bucketOf,
  collectTasks,
  daysBetween,
  escapeIcsText,
  foldIcsLine,
  groupByStatus,
  groupTasks,
  ganttRange,
  isBlocked,
  isDateKey,
  KANBAN_ORDER,
  monthGrid,
  parseDateKey,
  taskBar,
  taskKey,
  taskStats,
  taskStatus,
  taskTags,
  tasksOnDay,
  filterTasks,
  hasActiveTaskFilters,
  toDateKey,
  wouldCreateCycle,
} from '../planning';
import type { CapsuleNode, TodoItem } from '../../types';
import type { PlannedTask } from '../planning';

const capsule = (id: string, items: TodoItem[], tags: string[] = ['#work']): CapsuleNode => ({
  messageId: id,
  subject: `Capsule ${id}`,
  from: 'a@local',
  to: 'b@local',
  date: 'Thu, 22 Oct 2026 10:00:00 GMT',
  timestamp: 0,
  references: [],
  tags,
  status: 'Active',
  fileName: `${id}.eml`,
  blocks: [
    { id: `blk_${id}`, type: 'todo_list', data: { title: 'Tasks', items } },
    { id: `blk_${id}_p`, type: 'paragraph', data: { text: 'ignore me' } },
  ],
  attachments: [],
});

const task = (id: string, text: string, done = false, due?: string): PlannedTask => ({
  capsuleId: 'c1',
  capsuleSubject: 'Capsule c1',
  capsuleTags: ['#work'],
  blockId: 'blk_c1',
  blockTitle: 'Tasks',
  item: { id, text, done, due },
});

describe('calendar day keys', () => {
  it('formats local dates without timezone drift', () => {
    expect(toDateKey(new Date(2026, 9, 22, 23, 30))).toBe('2026-10-22');
    expect(toDateKey(new Date(2026, 0, 1, 0, 1))).toBe('2026-01-01');
  });

  it('rejects malformed or impossible dates', () => {
    expect(isDateKey('2026-10-22')).toBe(true);
    expect(isDateKey('2026-2-3')).toBe(false);
    expect(isDateKey('2026-02-31')).toBe(false);
    expect(isDateKey(undefined)).toBe(false);
    expect(parseDateKey('nope')).toBeNull();
  });

  it('adds days across month, year and DST boundaries', () => {
    expect(addDays('2026-10-31', 1)).toBe('2026-11-01');
    expect(addDays('2026-03-28', 1)).toBe('2026-03-29'); // EU DST switch
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01');
    expect(addDays('2026-01-01', -1)).toBe('2025-12-31');
  });

  it('counts whole days between keys', () => {
    expect(daysBetween('2026-10-22', '2026-10-22')).toBe(0);
    expect(daysBetween('2026-10-22', '2026-10-25')).toBe(3);
    expect(daysBetween('2026-10-22', '2026-10-21')).toBe(-1);
    expect(daysBetween('2026-03-28', '2026-03-30')).toBe(2); // DST-safe
  });
});

describe('month grid', () => {
  it('returns 42 Monday-first cells covering the month', () => {
    const cells = monthGrid(2026, 9); // October 2026 starts on Thursday
    expect(cells).toHaveLength(42);
    expect(cells[0]).toBe('2026-09-28'); // Monday before Oct 1
    expect(cells).toContain('2026-10-01');
    expect(cells).toContain('2026-10-31');
    expect(new Set(cells).size).toBe(42);
  });
});

describe('collectTasks', () => {
  it('walks every todo block and ignores other blocks', () => {
    const tasks = collectTasks([
      capsule('c1', [
        { id: 'i1', text: 'One', done: false },
        { id: 'i2', text: 'Two', done: true },
      ]),
      capsule('c2', [{ id: 'i3', text: 'Three', done: false }]),
    ]);
    expect(tasks.map((t) => t.item.id)).toEqual(['i1', 'i2', 'i3']);
    expect(tasks[0].capsuleId).toBe('c1');
    expect(tasks[0].blockTitle).toBe('Tasks');
  });

  it('survives capsules without todo blocks', () => {
    const empty: CapsuleNode = {
      ...capsule('c9', []),
      blocks: [{ id: 'p', type: 'paragraph', data: { text: 'x' } }],
    };
    expect(collectTasks([empty])).toEqual([]);
  });
});

describe('bucketing', () => {
  const today = '2026-10-22';

  it('maps due dates to agenda buckets', () => {
    expect(bucketOf(task('a', 'a', false, '2026-10-21'), today)).toBe('overdue');
    expect(bucketOf(task('b', 'b', false, '2026-10-22'), today)).toBe('today');
    expect(bucketOf(task('c', 'c', false, '2026-10-23'), today)).toBe('tomorrow');
    expect(bucketOf(task('d', 'd', false, '2026-10-25'), today)).toBe('week');
    expect(bucketOf(task('e', 'e', false, '2026-10-29'), today)).toBe('week');
    expect(bucketOf(task('f', 'f', false, '2026-10-30'), today)).toBe('later');
    expect(bucketOf(task('g', 'g', false), today)).toBe('none');
    expect(bucketOf(task('h', 'h', false, 'not-a-date'), today)).toBe('none');
  });

  it('completed tasks always land in done', () => {
    expect(bucketOf(task('a', 'a', true, '2020-01-01'), today)).toBe('done');
    expect(bucketOf(task('b', 'b', true), today)).toBe('done');
  });

  it('groups, sorts by due then priority, and drops empty sections', () => {
    const sections = groupTasks(
      [
        task('later', 'later', false, '2026-12-01'),
        task('todayLow', 'today low', false, '2026-10-22'),
        task('todayHigh', 'today high', false, '2026-10-22'),
        task('done', 'done', true),
      ],
      today,
    );
    expect(sections.map((s) => s.id)).toEqual(['today', 'later', 'done']);
    const todayIds = sections[0].tasks.map((t) => t.item.id);
    expect(todayIds).toContain('todayHigh');
    expect(todayIds).toContain('todayLow');
  });

  it('orders same-day tasks by priority', () => {
    const high = task('h', 'high', false, today);
    high.item.priority = 'high';
    const normal = task('n', 'normal', false, today);
    const low = task('l', 'low', false, today);
    low.item.priority = 'low';
    const [section] = groupTasks([low, normal, high], today);
    expect(section.tasks.map((t) => t.item.id)).toEqual(['h', 'n', 'l']);
  });
});

describe('stats and day filter', () => {
  const today = '2026-10-22';
  const tasks = [
    task('a', 'a', false, '2026-10-20'),
    task('b', 'b', false, today),
    task('c', 'c', true, today),
    task('d', 'd', false, '2026-11-30'),
  ];

  it('counts open, done, overdue and due today', () => {
    expect(taskStats(tasks, today)).toEqual({
      total: 4,
      done: 1,
      open: 3,
      overdue: 1,
      today: 1,
      completionRate: 25,
    });
  });

  it('handles an empty vault without dividing by zero', () => {
    expect(taskStats([], today)).toEqual({
      total: 0,
      done: 0,
      open: 0,
      overdue: 0,
      today: 0,
      completionRate: 0,
    });
  });

  it('filters one day, hiding completed tasks by default', () => {
    expect(tasksOnDay(tasks, today).map((t) => t.item.id)).toEqual(['b']);
    expect(tasksOnDay(tasks, today, true).map((t) => t.item.id)).toEqual(['b', 'c']);
  });
});

describe('ICS export (RFC 5545)', () => {
  it('escapes TEXT special characters', () => {
    expect(escapeIcsText('a,b;c\\d\ne')).toBe('a\\,b\\;c\\\\d\\ne');
  });

  it('folds long lines to 75 octets, never splitting UTF-8', () => {
    const long = 'Задача ' + 'я'.repeat(60) + ' end';
    const folded = foldIcsLine(long);
    expect(folded.length).toBeGreaterThan(1);
    for (const line of folded) {
      expect(new TextEncoder().encode(line).length).toBeLessThanOrEqual(75);
    }
    expect(folded.slice(1).every((l) => l.startsWith(' '))).toBe(true);
    // Joining must restore the original text exactly.
    expect(folded[0] + folded.slice(1).map((l) => l.slice(1)).join('')).toBe(long);
  });

  it('keeps short lines untouched', () => {
    expect(foldIcsLine('SUMMARY:short')).toEqual(['SUMMARY:short']);
  });

  it('emits one VTODO per open task with DUE, STATUS, PRIORITY', () => {
    const high = task('a', 'Ship it', false, '2026-10-22');
    high.item.priority = 'high';
    const ics = buildIcs([high, task('b', 'Done already', true)], {
      now: new Date(Date.UTC(2026, 9, 22, 8, 30, 0)),
    });

    expect(ics.startsWith('BEGIN:VCALENDAR\r\n')).toBe(true);
    expect(ics.endsWith('END:VCALENDAR\r\n')).toBe(true);
    expect(ics).toContain('DTSTAMP:20261022T083000Z');
    expect(ics).toContain('DUE;VALUE=DATE:20261022');
    expect(ics).toContain('SUMMARY:Ship it');
    expect(ics).toContain('STATUS:NEEDS-ACTION');
    expect(ics).toContain('PRIORITY:1');
    expect(ics.match(/BEGIN:VTODO/g)).toHaveLength(1); // completed excluded
    expect(ics.includes('\n') && !ics.includes('\r\n')).toBe(false);
  });

  it('can include completed tasks', () => {
    const ics = buildIcs([task('b', 'Done already', true, '2026-10-20')], {
      includeCompleted: true,
    });
    expect(ics).toContain('STATUS:COMPLETED');
    expect(ics).toContain('PERCENT-COMPLETE:100');
    expect(ics).toContain('DUE;VALUE=DATE:20261020');
  });

  it('escapes summary text and sanitises UIDs', () => {
    const ics = buildIcs([task('id with spaces', 'a,b; c\nnew line', false, '2026-10-22')]);
    expect(ics).toContain('SUMMARY:a\\,b\\; c\\nnew line');
    expect(ics).toContain('UID:blk_c1.id-with-spaces@capsuleos.local');
  });

  it('omits DUE for undated tasks', () => {
    const ics = buildIcs([task('x', 'someday', false)]);
    expect(ics).not.toContain('DUE');
    expect(ics).toContain('STATUS:NEEDS-ACTION');
  });
});

describe('kanban status', () => {  it('falls back to the legacy done flag when status is absent', () => {
    expect(taskStatus({ id: '1', text: 'a', done: false })).toBe('todo');
    expect(taskStatus({ id: '2', text: 'b', done: true })).toBe('done');
  });

  it('respects an explicit status over the done flag', () => {
    expect(taskStatus({ id: '1', text: 'a', done: false, status: 'doing' })).toBe('doing');
  });

  it('puts every task into exactly one fixed column', () => {
    const tasks = [
      task('a', 'todo task'),
      { ...task('b', 'doing task'), item: { ...task('b', 'doing task').item, status: 'doing' as const } },
      task('c', 'done task', true),
    ];
    const board = groupByStatus(tasks);
    expect(KANBAN_ORDER).toEqual(['todo', 'doing', 'done']);
    expect(board.todo.map((t) => t.item.id)).toEqual(['a']);
    expect(board.doing.map((t) => t.item.id)).toEqual(['b']);
    expect(board.done.map((t) => t.item.id)).toEqual(['c']);
  });
});

describe('gantt geometry', () => {
  it('pads the range by one day around dated tasks', () => {
    const tasks = [task('a', 'A', false, '2026-10-20'), task('b', 'B', false, '2026-10-25')];
    expect(ganttRange(tasks, '2026-10-22')).toEqual({
      start: '2026-10-19',
      end: '2026-10-26',
    });
  });

  it('falls back to a two-week window when nothing is dated', () => {
    expect(ganttRange([task('x', 'X')], '2026-10-22')).toEqual({
      start: '2026-10-21',
      end: '2026-11-04',
    });
  });

  it('positions a multi-day bar from start to due', () => {
    const multi = task('a', 'A', false, '2026-10-23');
    multi.item.start = '2026-10-20';
    expect(taskBar(multi, { start: '2026-10-20', end: '2026-10-30' })).toEqual({
      left: 0,
      span: 4,
    });
    expect(taskBar(task('b', 'B'), { start: '2026-10-20', end: '2026-10-30' })).toBeNull();
  });
});

describe('task dependencies', () => {
  const key = (id: string) => `blk_c1:${id}`;

  it('detects transitive cycles before an edge is added', () => {
    const a = task('a', 'A');
    const b = task('b', 'B');
    const c = task('c', 'C');
    a.item.blockedBy = [key('b')];
    b.item.blockedBy = [key('c')];
    const tasks = [a, b, c];
    expect(wouldCreateCycle(tasks, key('c'), key('a'))).toBe(true);
    expect(wouldCreateCycle(tasks, key('a'), key('c'))).toBe(false);
    expect(wouldCreateCycle(tasks, key('a'), key('a'))).toBe(true);
  });

  it('honors every entry of a multi-blocker array, not just the first', () => {
    // The Gantt editor can attach several prerequisites at once since the
    // multi-blocker select landed; blocking must consider them all.
    const a = task('a', 'A');
    const b = task('b', 'B');
    const c = task('c', 'C');
    a.item.blockedBy = [key('b'), key('c')];
    let tasks = [a, b, c];
    expect(isBlocked(a, tasks)).toBe(true);
    b.item.done = true;
    b.item.status = 'done';
    tasks = [a, b, c];
    expect(isBlocked(a, tasks)).toBe(true); // c still open
    c.item.done = true;
    c.item.status = 'done';
    tasks = [a, b, c];
    expect(isBlocked(a, tasks)).toBe(false);
  });

  it('marks a task blocked until every prerequisite is done', () => {
    const a = task('a', 'A');
    const b = task('b', 'B');
    a.item.blockedBy = [key('b')];
    let tasks = [a, b];
    expect(isBlocked(a, tasks)).toBe(true);
    b.item.done = true;
    b.item.status = 'done';
    tasks = [a, b];
    expect(isBlocked(a, tasks)).toBe(false);
  });
});

describe('cross-project task filters', () => {
  const today = '2026-10-22';

  const build = () => {
    const a = task('a', 'Write spec', false, '2026-10-20');
    a.item.status = 'todo';
    a.item.priority = 'high';

    const b = task('b', 'Ship build', false, '2026-10-22');
    b.item.status = 'doing';
    b.item.priority = 'normal';
    b.capsuleId = 'c2';
    b.capsuleSubject = 'Capsule c2';
    b.capsuleTags = ['#personal'];

    const c = task('c', 'Old chore', true, '2026-10-21');
    c.item.status = 'done';

    const d = task('d', 'Someday');
    d.item.status = 'todo';
    d.item.priority = 'low';

    return [a, b, c, d];
  };

  const ids = (tasks: PlannedTask[]) => tasks.map((t) => t.item.id);

  it('matches text across task, capsule and tags', () => {
    expect(ids(filterTasks(build(), { text: 'ship' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { text: 'personal' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { text: 'nothing' }, today))).toEqual([]);
  });

  it('filters by exact status and by open', () => {
    expect(ids(filterTasks(build(), { status: 'open' }, today))).toEqual(['a', 'b', 'd']);
    expect(ids(filterTasks(build(), { status: 'doing' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { status: 'done' }, today))).toEqual(['c']);
  });

  it('filters by priority, capsule and tag', () => {
    expect(ids(filterTasks(build(), { priority: 'high' }, today))).toEqual(['a']);
    expect(ids(filterTasks(build(), { priority: 'normal' }, today))).toEqual(['b', 'c']);
    expect(ids(filterTasks(build(), { capsuleId: 'c2' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { tag: '#personal' }, today))).toEqual(['b']);
  });

  it('filters due buckets without UTC drift', () => {
    expect(ids(filterTasks(build(), { due: 'overdue' }, today))).toEqual(['a']);
    expect(ids(filterTasks(build(), { due: 'today' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { due: 'week' }, today))).toEqual(['b']);
    expect(ids(filterTasks(build(), { due: 'none' }, today))).toEqual(['d']);
  });

  it('composes dimensions and reports active state', () => {
    expect(ids(filterTasks(build(), { status: 'todo', priority: 'low' }, today))).toEqual(['d']);
    expect(hasActiveTaskFilters({ status: 'all', priority: 'all', due: 'all' })).toBe(false);
    expect(hasActiveTaskFilters({ status: 'todo' })).toBe(true);
    expect(taskTags(build())).toEqual(['#personal', '#work']);
  });
});
