import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Search, CornerDownLeft, FileCode, Hash, ArrowUp, ArrowDown } from 'lucide-react';
import { vaultStore } from '../engine/vaultStore';
import {
  rankCommands,
  highlightMatches,
  type PaletteCommand,
  type HighlightSegment,
} from '../engine/commandPalette';
import { makeSnippet, blockPlainText } from '../engine/textUtils';
import type { CapsuleNode, ScoredSearchHit } from '../types';
import { useI18n } from '../i18n';

export interface PaletteAction extends PaletteCommand {
  run: () => void;
}

interface Props {
  isOpen: boolean;
  onClose: () => void;
  actions: PaletteAction[];
  onOpenCapsule: (messageId: string) => void;
  onImportEml: (file: File) => void;
}

type Item =
  | { kind: 'action'; action: PaletteAction }
  | { kind: 'capsule'; hit: ScoredSearchHit };

const Highlighted: React.FC<{ segments: HighlightSegment[] }> = ({ segments }) => (
  <>
    {segments.map((s, i) =>
      s.hit ? (
        <mark key={i} className="bg-cyan-500/25 text-cyan-200 rounded-[2px] px-0.5">
          {s.text}
        </mark>
      ) : (
        <span key={i}>{s.text}</span>
      ),
    )}
  </>
);

const queryTerms = (q: string): string[] =>
  q
    .toLowerCase()
    .split(/\s+/)
    .filter((t) => t.length > 1);

export const CommandPalette: React.FC<Props> = ({
  isOpen,
  onClose,
  actions,
  onOpenCapsule,
  onImportEml,
}) => {
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const { t } = useI18n();

  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setActiveIndex(0);
      // Focus after mount so the caret lands in the palette input.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [isOpen]);

  // The file picker belongs to the palette itself: it is the only way to offer
  // "Import .eml…" as a real palette command without duplicating input refs.
  const allActions = useMemo<PaletteAction[]>(
    () => [
      ...actions,
      {
        id: 'import-eml',
        title: t('palette.importTitle'),
        hint: t('palette.importHint'),
        group: t('palette.importGroup'),
        keywords: t('palette.import.keywords').split(' '),
        run: () => fileRef.current?.click(),
      },
    ],
    [actions, t],
  );

  const matchedActions = useMemo(
    () => rankCommands(query, allActions, 40).map((r) => r.command),
    [query, allActions],
  );

  // Capsules: BM25-ranked by the same engine that powers the sidebar.
  const capsuleHits = useMemo(() => {
    if (!query.trim()) {
      return vaultStore
        .getNodes()
        .slice(0, 4)
        .map((node) => ({ node, score: 0, matchedTerms: [] as string[] }));
    }
    return vaultStore.searchWithScores(query).slice(0, 6);
  }, [query, isOpen]);

  const items: Item[] = useMemo(
    () => [
      ...matchedActions.map((action) => ({ kind: 'action' as const, action })),
      ...capsuleHits.map((hit) => ({ kind: 'capsule' as const, hit })),
    ],
    [matchedActions, capsuleHits],
  );

  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  const runItem = (item: Item) => {
    if (item.kind === 'action') {
      onClose();
      // Let the palette unmount before the action steals focus / opens a dialog.
      requestAnimationFrame(() => item.action.run());
      return;
    }
    onClose();
    onOpenCapsule(item.hit.node.messageId);
  };

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIndex((i) => (items.length ? (i + 1) % items.length : 0));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIndex((i) => (items.length ? (i - 1 + items.length) % items.length : 0));
      } else if (e.key === 'Enter') {
        const item = items[activeIndex];
        if (!item) return;
        e.preventDefault();
        runItem(item);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen, items, activeIndex, onClose]);

  if (!isOpen) return null;

  const terms = queryTerms(query);
  const actionGroups: Array<{ group: string; items: PaletteAction[] }> = [];
  for (const action of matchedActions) {
    const bucket = actionGroups.find((g) => g.group === action.group);
    if (bucket) bucket.items.push(action);
    else actionGroups.push({ group: action.group, items: [action] });
  }

  const capsuleOffset = matchedActions.length;
  const capsuleTitle = query.trim() ? t('palette.capsules') : t('palette.recent');

  const capsuleSnippet = (node: CapsuleNode): HighlightSegment[] => {
    const text = `${node.subject}\n${blockPlainText(node.blocks)}`;
    return makeSnippet(text, terms.length ? terms : [], 80);
  };

  return (
    <div
      className="fixed inset-0 z-[60] bg-black/70 backdrop-blur-sm flex items-start justify-center pt-[12vh] px-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
    >
      <div className="w-full max-w-2xl rounded-2xl border border-cyan-500/30 bg-[#0b1220]/95 shadow-2xl shadow-cyan-950/40 overflow-hidden backdrop-blur-md">
        {/* Search input */}
        <div className="flex items-center gap-3 px-4 py-3 border-b border-slate-800">
          <Search className="w-4 h-4 text-cyan-400 shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('palette.placeholder')}
            aria-label={t('palette.aria')}
            className="flex-1 bg-transparent text-sm text-slate-100 placeholder:text-slate-500 outline-none"
          />
          <span className="text-[10px] font-mono text-slate-500 border border-slate-700 rounded px-1.5 py-0.5">
            esc
          </span>
        </div>

        {/* Results */}
        <div className="max-h-[56vh] overflow-y-auto py-2">
          {actionGroups.map((group) => (
            <div key={group.group} className="mb-1">
              <div className="px-4 py-1 text-[10px] font-mono uppercase tracking-wider text-slate-500">
                {group.group}
              </div>
              {group.items.map((action) => {
                const index = items.findIndex((i) => i.kind === 'action' && i.action.id === action.id);
                const active = index === activeIndex;
                return (
                  <button
                    key={action.id}
                    type="button"
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => runItem({ kind: 'action', action })}
                    className={`w-full flex items-center gap-3 px-4 py-2 text-left transition-colors ${
                      active ? 'bg-cyan-500/10 text-cyan-100' : 'text-slate-300 hover:bg-slate-800/40'
                    }`}
                  >
                    <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${active ? 'bg-cyan-400' : 'bg-slate-700'}`} />
                    <span className="flex-1 min-w-0">
                      <span className="block text-xs font-medium truncate">{action.title}</span>
                      {action.hint && (
                        <span className="block text-[10px] text-slate-500 truncate">
                          {action.hint}
                        </span>
                      )}
                    </span>
                    {action.shortcut && (
                      <span className="text-[10px] font-mono text-slate-500 bg-slate-900 border border-slate-700 rounded px-1.5 py-0.5">
                        {action.shortcut}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))}

          {capsuleHits.length > 0 && (
            <div>
              <div className="px-4 py-1 text-[10px] font-mono uppercase tracking-wider text-slate-500 flex items-center justify-between">
                <span>{capsuleTitle}</span>
                {query.trim() && (
                  <span className="text-slate-600 normal-case">
                    {t('palette.matches', { count: vaultStore.searchWithScores(query).length })}
                  </span>
                )}
              </div>
              {capsuleHits.map((hit, i) => {
                const index = capsuleOffset + i;
                const active = index === activeIndex;
                return (
                  <button
                    key={hit.node.messageId}
                    type="button"
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => runItem({ kind: 'capsule', hit })}
                    className={`w-full flex items-start gap-3 px-4 py-2 text-left transition-colors ${
                      active ? 'bg-cyan-500/10' : 'hover:bg-slate-800/40'
                    }`}
                  >
                    <FileCode
                      className={`w-3.5 h-3.5 shrink-0 mt-0.5 ${active ? 'text-cyan-400' : 'text-slate-500'}`}
                    />
                    <span className="flex-1 min-w-0">
                      <span
                        className={`block text-xs font-medium truncate ${
                          active ? 'text-cyan-100' : 'text-slate-200'
                        }`}
                      >
                        <Highlighted segments={highlightMatches(hit.node.subject, query)} />
                      </span>
                      <span className="block text-[10px] text-slate-500 line-clamp-2 leading-snug">
                        <Highlighted segments={capsuleSnippet(hit.node)} />
                      </span>
                    </span>
                    <span className="flex items-center gap-1.5 text-[10px] font-mono text-slate-500 shrink-0 mt-0.5">
                      {hit.node.tags.slice(0, 1).map((t) => (
                        <span key={t} className="inline-flex items-center gap-0.5">
                          <Hash className="w-2.5 h-2.5" />
                          {t.replace(/^#/, '')}
                        </span>
                      ))}
                      {hit.score > 0 && <span className="text-cyan-400/80">Σ{hit.score.toFixed(1)}</span>}
                    </span>
                  </button>
                );
              })}
            </div>
          )}

          {items.length === 0 && (
            <div className="px-4 py-10 text-center text-xs text-slate-500 font-mono">
              {t('palette.empty')}
            </div>
          )}
        </div>

        {/* Footer hints */}
        <div className="flex items-center justify-between px-4 py-2 border-t border-slate-800 text-[10px] font-mono text-slate-500">
          <span className="flex items-center gap-3">
            <span className="inline-flex items-center gap-1">
              <ArrowUp className="w-3 h-3" />
              <ArrowDown className="w-3 h-3" /> {t('palette.navigate')}
            </span>
            <span className="inline-flex items-center gap-1">
              <CornerDownLeft className="w-3 h-3" /> {t('palette.open')}
            </span>
          </span>
          <span>
            {items.length === 1
              ? t('palette.resultOne', { count: items.length })
              : t('palette.resultMany', { count: items.length })}
          </span>
        </div>
      </div>

      <input
        ref={fileRef}
        type="file"
        accept=".eml,message/rfc822"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) onImportEml(file);
          e.target.value = '';
        }}
      />
    </div>
  );
};
