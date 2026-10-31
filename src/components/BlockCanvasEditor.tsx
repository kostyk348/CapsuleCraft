import React, { useState, useEffect } from 'react';
import {
  CapsuleNode,
  CapsuleBlock,
  BlockType,
  CapsuleAttachment,
  AudioNoteBlockData,
} from '../types';
import { HeadingBlock } from './blocks/HeadingBlock';
import { RichTextBlock } from './blocks/RichTextBlock';
import { AiOpponentBlock } from './blocks/AiOpponentBlock';
import { DataGridBlock } from './blocks/DataGridBlock';
import { WhiteboardBlock } from './blocks/WhiteboardBlock';
import { CodeReplBlock } from './blocks/CodeReplBlock';
import { AudioNoteBlock } from './blocks/AudioNoteBlock';
import { CalloutBlock } from './blocks/CalloutBlock';
import { TodoBlock } from './blocks/TodoBlock';
import { SlashCommandMenu } from './SlashCommandMenu';
import { vaultStore } from '../engine/vaultStore';
import { isDesktop, pickSavePath, writeTextFile } from '../engine/desktopStore';
import { shouldOpenSlashMenu } from '../engine/textUtils';
import {
  nodeMap,
  pageBreadcrumbs,
  pageDescendantIds,
} from '../engine/hierarchy';
import { useI18n } from '../i18n';
import {
  Sparkles,
  Plus,
  ArrowUp,
  ArrowDown,
  Trash2,
  Paperclip,
  Tag,
  CornerDownRight,
  ShieldAlert,
  Bot,
  Layers,
  FileCode,
  FileSpreadsheet,
  Palette,
  ChevronDown,
  ChevronUp,
  ChevronRight,
  FilePlus,
  FolderTree,
} from 'lucide-react';

interface Props {
  capsule: CapsuleNode;
  allCapsules: CapsuleNode[];
  onUpdateCapsule: (updated: CapsuleNode) => void;
  onOpenParent: (parentId: string) => void;
  onReparent?: (parentId: string | null) => void;
  onCreateChild?: () => void;
  onRequestAiAction: (action: 'critique' | 'continue' | 'reply_thread' | 'data_insight') => void;
  isAiLoading?: boolean;
  onOpenCapsule?: (messageId: string) => void;
}

export const BlockCanvasEditor: React.FC<Props> = ({
  capsule,
  allCapsules,
  onUpdateCapsule,
  onOpenParent,
  onReparent,
  onCreateChild,
  onRequestAiAction,
  isAiLoading,
  onOpenCapsule,
}) => {
  const [showMeta, setShowMeta] = useState(true);
  const [slashMenuOpen, setSlashMenuOpen] = useState(false);
  const [slashSource, setSlashSource] = useState<{ index: number; text: string } | null>(null);
  const [tagInput, setTagInput] = useState('');
  const [hoveredBlockIndex, setHoveredBlockIndex] = useState<number | null>(null);

  const { t } = useI18n();

  const pageNodes = nodeMap(allCapsules);
  const breadcrumbs = pageBreadcrumbs(pageNodes, capsule.messageId);
  const currentParentId =
    capsule.parentId && pageNodes.has(capsule.parentId) && capsule.parentId !== capsule.messageId
      ? capsule.parentId
      : '';
  const descendantIds = pageDescendantIds(pageNodes, capsule.messageId);
  const parentOptions = allCapsules
    .filter(
      (candidate) =>
        candidate.messageId !== capsule.messageId && !descendantIds.has(candidate.messageId),
    )
    .sort((a, b) => (a.subject || '').localeCompare(b.subject || ''));

  const handleExportMarkdown = () => exportCapsuleMarkdown(capsule);

  // Keyboard shortcut listener: Alt+C -> Critique, Alt+E -> Expand/continue.
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.altKey && (e.key === 'c' || e.key === 'C' || e.key === 'с' || e.key === 'С')) {
        e.preventDefault();
        onRequestAiAction('critique');
      } else if (e.altKey && (e.key === 'e' || e.key === 'E' || e.key === 'у' || e.key === 'У')) {
        e.preventDefault();
        onRequestAiAction('continue');
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onRequestAiAction]);

  // Updates
  const updateBlock = (index: number, updatedBlock: CapsuleBlock) => {
    const newBlocks = [...capsule.blocks];
    newBlocks[index] = updatedBlock;
    onUpdateCapsule({ ...capsule, blocks: newBlocks });
  };

  const deleteBlock = (index: number) => {
    const newBlocks = capsule.blocks.filter((_, i) => i !== index);
    onUpdateCapsule({ ...capsule, blocks: newBlocks });
  };

  const moveBlock = (index: number, direction: 'up' | 'down') => {
    const targetIdx = direction === 'up' ? index - 1 : index + 1;
    if (targetIdx < 0 || targetIdx >= capsule.blocks.length) return;
    const newBlocks = [...capsule.blocks];
    const temp = newBlocks[index];
    newBlocks[index] = newBlocks[targetIdx];
    newBlocks[targetIdx] = temp;
    onUpdateCapsule({ ...capsule, blocks: newBlocks });
  };

  const insertBlock = (type: BlockType, extraData?: any, atIndex?: number) => {
    const id = `blk_${Date.now().toString(36)}_${Math.random().toString(36).substring(2, 6)}`;
    let newBlock: CapsuleBlock;

    switch (type) {
      case 'heading':
        newBlock = {
          id,
          type: 'heading',
          data: extraData || { level: 2, text: '' },
        };
        break;
      case 'paragraph':
        newBlock = {
          id,
          type: 'paragraph',
          data: extraData || { text: '' },
        };
        break;
      case 'ai_opponent_block':
        newBlock = {
          id,
          type: 'ai_opponent_block',
          author: 'Co-Thinker',
          data: extraData || { criticism: '' },
        };
        break;
      case 'data_grid_view': {
        const sourceCid = extraData?.source_cid || `cid:dataset_${Date.now().toString(36)}_csv`;
        const cleanCid = sourceCid.replace(/^cid:/, '');
        // Also attach an empty CSV so the block is addressable; the DataGrid
        // shows an honest empty state until a real file is picked.
        const newAttachment: CapsuleAttachment = {
          name: `${cleanCid}.csv`,
          cid: cleanCid,
          mimeType: 'text/csv',
          content: '',
        };
        newBlock = {
          id,
          type: 'data_grid_view',
          data: {
            source_cid: sourceCid,
            view: 'table',
            title: '',
          },
        };
        onUpdateCapsule({
          ...capsule,
          attachments: [...capsule.attachments, newAttachment],
          blocks: [...capsule.blocks, newBlock],
        });
        return;
      }
      case 'interactive_canvas': {
        const sceneCid = extraData?.scene_cid || `cid:whiteboard_${Date.now().toString(36)}_json`;
        const cleanCid = sceneCid.replace(/^cid:/, '');
        const newAttachment: CapsuleAttachment = {
          name: `${cleanCid}.json`,
          cid: cleanCid,
          mimeType: 'application/json',
          content: JSON.stringify({ elements: [] }),
        };
        newBlock = {
          id,
          type: 'interactive_canvas',
          data: { scene_cid: sceneCid },
        };
        onUpdateCapsule({
          ...capsule,
          attachments: [...capsule.attachments, newAttachment],
          blocks: [...capsule.blocks, newBlock],
        });
        return;
      }
      case 'code_repl':
        newBlock = {
          id,
          type: 'code_repl',
          data: extraData || {
            language: 'javascript',
            code: '',
          },
        };
        break;
      case 'audio_note':
        newBlock = {
          id,
          type: 'audio_note',
          data: extraData || {
            duration: '0:00',
            transcript: '',
            waveform: [],
            author: 'User Voice',
          },
        };
        break;
      case 'callout':
        newBlock = {
          id,
          type: 'callout',
          data: extraData || {
            title: '',
            content: '',
            tone: 'emerald',
          },
        };
        break;
      case 'todo_list':
        newBlock = {
          id,
          type: 'todo_list',
          data: extraData || {
            title: '',
            items: [{ id: `td_${Date.now().toString(36)}`, text: '', done: false, status: 'todo' }],
          },
        };
        break;
      default:
        newBlock = {
          id,
          type: 'paragraph',
          data: { text: '' },
        };
    }

    const insertIdx = atIndex !== undefined ? atIndex : capsule.blocks.length;
    const newBlocks = [...capsule.blocks];
    newBlocks.splice(insertIdx, 0, newBlock);
    onUpdateCapsule({ ...capsule, blocks: newBlocks });
  };

  /**
   * `/task` converts the paragraph the menu was opened from into a todo block:
   * every non-empty line becomes one trackable task. With no text it inserts a
   * single empty task. Other commands keep their normal insert behaviour.
   */
  const handleSlashSelect = (type: BlockType, extraData?: any, commandId?: string) => {
    if (commandId === 'task') {
      const sourceText = slashSource?.text.trim() || '';
      const lines = sourceText ? sourceText.split(/\n+/).map((line) => line.trim()).filter(Boolean) : [''];
      const items = lines.map((text, i) => ({
        id: `td_${Date.now().toString(36)}_${i}`,
        text,
        done: false,
        status: 'todo' as const,
      }));
      if (slashSource && sourceText) {
        const taskBlock: CapsuleBlock = {
          id: `blk_task_${Date.now().toString(36)}`,
          type: 'todo_list',
          data: { items },
        };
        onUpdateCapsule({
          ...capsule,
          blocks: capsule.blocks.map((b, i) => (i === slashSource.index ? taskBlock : b)),
        });
      } else {
        insertBlock('todo_list', { items });
      }
      setSlashSource(null);
      setSlashMenuOpen(false);
      return;
    }
    insertBlock(type, extraData);
    setSlashSource(null);
  };

  const handleUpdateAttachment = (cid: string, newContent: string, mimeType?: string, name?: string) => {
    const updatedAttachments = capsule.attachments.map((att) =>
      att.cid === cid
        ? { ...att, content: newContent, mimeType: mimeType || att.mimeType, name: name || att.name, sizeBytes: newContent.length }
        : att
    );
    // If not found, add
    if (!updatedAttachments.some((a) => a.cid === cid)) {
      updatedAttachments.push({
        name: name || `${cid}.dat`,
        cid,
        mimeType: mimeType || 'text/plain',
        content: newContent,
        sizeBytes: newContent.length,
      });
    }
    onUpdateCapsule({ ...capsule, attachments: updatedAttachments });
  };

  /**
   * Atomic audio attachment: adds/updates the MIME attachment and the
   * block's audioCid/audioMime in ONE capsule update (two sequential
   * updates would clobber each other via the stale closure snapshot).
   */
  const handleAttachAudio = (
    blockIndex: number,
    blockBlock: CapsuleBlock & { type: 'audio_note' },
    cid: string,
    content: string,
    mimeType: string,
    name: string,
    extraData?: Partial<AudioNoteBlockData>
  ) => {
    const exists = capsule.attachments.some((a) => a.cid === cid);
    const attachments = exists
      ? capsule.attachments.map((a) =>
          a.cid === cid ? { ...a, content, mimeType, name, sizeBytes: content.length } : a
        )
      : [...capsule.attachments, { name, cid, mimeType, content, sizeBytes: content.length }];
    // Atomic: attachment + block link + optional block data (duration, waveform)
    // merged in ONE capsule update — extra updates from stale block snapshots
    // would silently drop the audioCid link.
    const newBlocks = [...capsule.blocks];
    newBlocks[blockIndex] = {
      ...blockBlock,
      data: { ...blockBlock.data, audioCid: cid, audioMime: mimeType, ...extraData },
    };
    onUpdateCapsule({ ...capsule, attachments, blocks: newBlocks });
  };

  const handleAddTag = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && tagInput.trim()) {
      const cleanTag = tagInput.trim().startsWith('#') ? tagInput.trim() : `#${tagInput.trim()}`;
      if (!capsule.tags.includes(cleanTag)) {
        onUpdateCapsule({ ...capsule, tags: [...capsule.tags, cleanTag] });
      }
      setTagInput('');
    }
  };

  const handleRemoveTag = (tagToRemove: string) => {
    onUpdateCapsule({
      ...capsule,
      tags: capsule.tags.filter((t) => t !== tagToRemove),
    });
  };

  return (
    <div className="w-full h-full bg-[#020617] overflow-y-auto flex flex-col items-center py-6 px-4 md:px-8 relative selection:bg-cyan-500/30">
      <div className="w-full max-w-4xl space-y-6">
        {/* RFC 5322 Metadata Card */}
        <div className="rounded-2xl border border-slate-800 bg-[#0f172a]/30 shadow-2xl overflow-hidden backdrop-blur-md">
          <div className="p-5 sm:p-6">
            {/* Message ID Header */}
            <div className="text-[10px] font-mono text-slate-500 mb-1.5 flex items-center gap-2">
              <span className="text-cyan-400 font-bold">MESSAGE-ID:</span>
              <span className="truncate">{capsule.messageId}</span>
            </div>

            {/* Page breadcrumb (wiki hierarchy, separate from mail threading) */}
            <div className="flex flex-wrap items-center gap-1 text-[11px] font-mono text-slate-500 mb-2">
              <span className="uppercase tracking-wider text-slate-600">
                {t('editor.vaultRoot')}
              </span>
              {breadcrumbs.map((ref, index) => {
                const isCurrent = index === breadcrumbs.length - 1;
                return (
                  <React.Fragment key={ref.messageId}>
                    <ChevronRight className="w-3 h-3 text-slate-700 shrink-0" />
                    {isCurrent ? (
                      <span className="text-slate-300 truncate max-w-[16rem]">
                        {ref.subject || t('editor.untitled')}
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => onOpenCapsule?.(ref.messageId)}
                        className="hover:text-cyan-300 truncate max-w-[12rem] transition-colors"
                      >
                        {ref.subject || t('editor.untitled')}
                      </button>
                    )}
                  </React.Fragment>
                );
              })}
            </div>

            {/* Subject Title */}
            <input
              type="text"
              value={capsule.subject}
              onChange={(e) => onUpdateCapsule({ ...capsule, subject: e.target.value })}
              placeholder="Capsule Subject (RFC 5322)..."
              className="w-full bg-transparent text-2xl sm:text-3xl font-bold text-white placeholder:text-slate-600 outline-none border-none focus:ring-0 tracking-tight mb-3"
            />

            {/* Quick Badges & Toggle */}
            <div className="flex flex-wrap items-center justify-between gap-3 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                {/* Status selector */}
                <select
                  value={capsule.status}
                  onChange={(e) => onUpdateCapsule({ ...capsule, status: e.target.value as any })}
                  className="bg-slate-800 text-cyan-400 font-mono font-semibold text-[11px] uppercase border border-slate-700 rounded-md px-2.5 py-1 outline-none"
                >
                  <option value="Active">Active</option>
                  <option value="Synthesis">Synthesis</option>
                  <option value="Draft">Draft</option>
                  <option value="Archived">Archived</option>
                </select>

                {/* Wiki parent selector — cycle-safe candidates only */}
                {onReparent && (
                  <label
                    className="inline-flex items-center gap-1 text-[11px] font-mono text-cyan-300 bg-cyan-950/40 border border-cyan-800/50 rounded-md px-2 py-1"
                    title={t('editor.parentLabel')}
                  >
                    <FolderTree className="w-3 h-3 text-cyan-400" />
                    <select
                      value={currentParentId}
                      onChange={(e) => onReparent(e.target.value || null)}
                      className="bg-transparent text-cyan-200 outline-none max-w-[13rem] text-[11px] font-mono cursor-pointer"
                    >
                      <option value="" className="bg-slate-900 text-slate-300">
                        {t('editor.noParent')}
                      </option>
                      {parentOptions.map((candidate) => (
                        <option
                          key={candidate.messageId}
                          value={candidate.messageId}
                          className="bg-slate-900 text-slate-200"
                        >
                          {candidate.subject || t('editor.untitled')}
                        </option>
                      ))}
                    </select>
                  </label>
                )}

                {onCreateChild && (
                  <button
                    type="button"
                    onClick={onCreateChild}
                    className="inline-flex items-center gap-1 text-[11px] font-mono text-emerald-300 bg-emerald-950/40 hover:bg-emerald-900/50 px-2.5 py-1 rounded-md border border-emerald-800/60 transition-colors"
                  >
                    <FilePlus className="w-3 h-3 text-emerald-400" />
                    <span>{t('editor.newSubpage')}</span>
                  </button>
                )}

                {/* Mail thread parent (In-Reply-To) if present */}
                {capsule.inReplyTo && (
                  <button
                    type="button"
                    onClick={() => onOpenParent(capsule.inReplyTo!)}
                    className="inline-flex items-center gap-1 text-[11px] font-mono text-slate-300 bg-slate-800/60 hover:bg-slate-700/60 px-2.5 py-1 rounded-md border border-slate-700/60 transition-colors"
                  >
                    <CornerDownRight className="w-3 h-3 text-slate-400" />
                    <span>
                      {t('editor.replyTo')}: {capsule.inReplyTo.substring(0, 14)}…
                    </span>
                  </button>
                )}

                {/* Tags List */}
                {capsule.tags.map((t) => (
                  <span
                    key={t}
                    className="group/tag inline-flex items-center gap-1 text-[10px] font-mono text-slate-400 bg-slate-800 px-2 py-0.5 rounded border border-slate-700"
                  >
                    <span>{t}</span>
                    <button
                      type="button"
                      onClick={() => handleRemoveTag(t)}
                      className="text-slate-500 hover:text-rose-400 opacity-60 group-hover/tag:opacity-100"
                    >
                      ×
                    </button>
                  </span>
                ))}

                <input
                  type="text"
                  value={tagInput}
                  onChange={(e) => setTagInput(e.target.value)}
                  onKeyDown={handleAddTag}
                  placeholder="+ #tag"
                  className="bg-transparent text-[10px] font-mono text-slate-400 placeholder:text-slate-600 outline-none w-20"
                />
              </div>

              <button
                type="button"
                onClick={() => setShowMeta(!showMeta)}
                className="flex items-center gap-1 text-slate-500 hover:text-cyan-400 text-xs font-mono transition-colors"
              >
                <span>{showMeta ? 'Hide RFC 5322 Headers' : 'Show RFC 5322 Headers'}</span>
                {showMeta ? <ChevronUp className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
              </button>
            </div>

            {/* Detailed Collapsible Header Grid */}
            {showMeta && (
              <div className="mt-4 pt-3 border-t border-slate-800/80 grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs font-mono text-slate-400 bg-slate-950/60 p-3 rounded-xl border border-slate-800">
                <div>
                  <span className="text-slate-500">From: </span>
                  <input
                    type="text"
                    value={capsule.from}
                    onChange={(e) => onUpdateCapsule({ ...capsule, from: e.target.value })}
                    className="bg-transparent text-slate-200 outline-none border-b border-transparent focus:border-cyan-500 w-44"
                  />
                </div>
                <div>
                  <span className="text-slate-500">To: </span>
                  <input
                    type="text"
                    value={capsule.to}
                    onChange={(e) => onUpdateCapsule({ ...capsule, to: e.target.value })}
                    className="bg-transparent text-slate-200 outline-none border-b border-transparent focus:border-cyan-500 w-44"
                  />
                </div>
                <div className="truncate">
                  <span className="text-slate-500">Message-ID: </span>
                  <span className="text-slate-300">{capsule.messageId}</span>
                </div>
                <div>
                  <span className="text-slate-500">Date: </span>
                  <span className="text-slate-300">{capsule.date}</span>
                </div>
              </div>
            )}
          </div>

          {/* AI Co-Thinking Instant Action Bar */}
          <div className="bg-[#0f172a]/60 border-t border-slate-800 px-5 py-3 flex flex-wrap items-center justify-between gap-3 text-xs">
            <div className="flex items-center gap-2 text-slate-400">
              <Sparkles className="w-4 h-4 text-cyan-400" />
              <span className="font-semibold text-white">AI Co-Thinker:</span>
              <span className="hidden sm:inline text-slate-500 font-mono text-[10px]">
                (Alt+C — Risk Critique | Alt+E — Expand Thesis)
              </span>
            </div>

            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={isAiLoading}
                onClick={() => onRequestAiAction('critique')}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-cyan-500/10 text-cyan-300 hover:bg-cyan-500/20 border border-cyan-500/30 text-xs font-medium transition-all shadow-[0_0_10px_rgba(6,182,212,0.15)]"
              >
                <ShieldAlert className="w-3.5 h-3.5 text-cyan-400" />
                <span>{isAiLoading ? 'Analyzing...' : 'Risk Critique (Alt+C)'}</span>
              </button>

              <button
                type="button"
                disabled={isAiLoading}
                onClick={() => onRequestAiAction('continue')}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-800 text-slate-200 hover:bg-slate-700 border border-slate-700 text-xs font-medium transition-all"
              >
                <Bot className="w-3.5 h-3.5 text-cyan-400" />
                <span>Continue Thought</span>
              </button>

              <button
                type="button"
                disabled={isAiLoading}
                onClick={() => onRequestAiAction('reply_thread')}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-purple-500/10 text-purple-300 hover:bg-purple-500/20 border border-purple-500/30 text-xs font-medium transition-all"
              >
                <CornerDownRight className="w-3.5 h-3.5" />
                <span>Create Re: .eml</span>
              </button>

              <button
                type="button"
                onClick={handleExportMarkdown}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-slate-800/80 text-emerald-300 hover:bg-emerald-950/40 border border-emerald-600/30 text-xs font-medium transition-all"
                title={t('editor.exportMd')}
              >
                <FileCode className="w-3.5 h-3.5 text-emerald-400" />
                <span>Export .md</span>
              </button>
            </div>
          </div>
        </div>

        {/* Backlinks Panel */}
        <RelatedCapsules
          capsule={capsule}
          onOpenCapsule={onOpenCapsule}
        />

        {/* Blocks Flow Container */}
        <div className="space-y-4 pb-16">
          {capsule.blocks.map((block, index) => (
            <div
              key={block.id}
              onMouseEnter={() => setHoveredBlockIndex(index)}
              onMouseLeave={() => setHoveredBlockIndex(null)}
              className="relative group/block transition-all"
            >
              {/* Block Action Controls on Hover */}
              <div className="absolute -left-10 top-2 opacity-0 group-hover/block:opacity-100 flex flex-col items-center gap-0.5 bg-slate-900/95 p-1 rounded-lg border border-slate-800 backdrop-blur-sm z-10 transition-opacity shadow-lg">
                <button
                  type="button"
                  onClick={() => moveBlock(index, 'up')}
                  disabled={index === 0}
                  className="p-1 text-slate-500 hover:text-cyan-400 disabled:opacity-30"
                  title="Move Up"
                >
                  <ArrowUp className="w-3 h-3" />
                </button>
                <button
                  type="button"
                  onClick={() => moveBlock(index, 'down')}
                  disabled={index === capsule.blocks.length - 1}
                  className="p-1 text-slate-500 hover:text-cyan-400 disabled:opacity-30"
                  title="Move Down"
                >
                  <ArrowDown className="w-3 h-3" />
                </button>
                <button
                  type="button"
                  onClick={() => deleteBlock(index)}
                  className="p-1 text-slate-500 hover:text-rose-400"
                  title="Delete Block"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>

              {/* Render Specific Block */}
              {block.type === 'heading' && (
                <HeadingBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                  onOpenCapsule={onOpenCapsule}
                />
              )}

              {block.type === 'paragraph' && (
                <RichTextBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onKeyDown={(e) => {
                    if (e.key === '/') {
                      const ta = e.currentTarget;
                      const caret = ta.selectionStart ?? ta.value.length;
                      // Notion-style: open the menu only at the start of a line
                      // or after whitespace — never for "and/or", dates or URLs.
                      if (shouldOpenSlashMenu(ta.value, caret)) {
                        e.preventDefault();
                        setSlashSource({ index, text: ta.value });
                        setSlashMenuOpen(true);
                      }
                    }
                  }}
                  onDelete={() => deleteBlock(index)}
                  onOpenCapsule={onOpenCapsule}
                />
              )}

              {block.type === 'ai_opponent_block' && (
                <AiOpponentBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                  onAcceptAsSynthesis={() => {
                    const newCallout: CapsuleBlock = {
                      id: `blk_syn_${Date.now()}`,
                      type: 'callout',
                      data: {
                        title: `Synthesis (${block.author || 'AI'})`,
                        content: block.data.criticism,
                        tone: 'purple',
                      },
                    };
                    updateBlock(index, newCallout);
                  }}
                />
              )}

              {block.type === 'data_grid_view' && (
                <DataGridBlock
                  block={block}
                  attachments={capsule.attachments}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onUpdateAttachment={handleUpdateAttachment}
                  onDelete={() => deleteBlock(index)}
                />
              )}

              {block.type === 'interactive_canvas' && (
                <WhiteboardBlock
                  block={block}
                  attachments={capsule.attachments}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onUpdateAttachment={handleUpdateAttachment}
                  onDelete={() => deleteBlock(index)}
                />
              )}

              {block.type === 'code_repl' && (
                <CodeReplBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                />
              )}

              {block.type === 'audio_note' && (
                <AudioNoteBlock
                  block={block}
                  attachments={capsule.attachments}
                  onUpdateAttachment={handleUpdateAttachment}
                  onAttachAudio={(cid, content, mimeType, name, extraData) =>
                    handleAttachAudio(index, block, cid, content, mimeType, name, extraData)
                  }
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                />
              )}

              {block.type === 'callout' && (
                <CalloutBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                  onOpenCapsule={onOpenCapsule}
                />
              )}

              {block.type === 'todo_list' && (
                <TodoBlock
                  block={block}
                  onChange={(d) => updateBlock(index, { ...block, data: d })}
                  onDelete={() => deleteBlock(index)}
                />
              )}
            </div>
          ))}

          {/* Add Block Trigger Button */}
          <div className="pt-2 flex items-center justify-center">
            <button
              type="button"
              onClick={() => {
                setSlashSource(null);
                setSlashMenuOpen(true);
              }}
              className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl bg-slate-900/60 hover:bg-slate-850 text-slate-400 hover:text-cyan-300 border border-dashed border-slate-700 hover:border-cyan-500/40 text-xs font-medium transition-all shadow-[0_0_10px_rgba(0,0,0,0.3)]"
            >
              <Plus className="w-4 h-4 text-cyan-400" />
              <span>Add block or type / (/table, /canvas, /ai-critique)</span>
            </button>
          </div>
        </div>

        {/* Slash Command Palette Menu */}
        <SlashCommandMenu
          isOpen={slashMenuOpen}
          onClose={() => setSlashMenuOpen(false)}
          onSelect={(type, extraData, commandId) => handleSlashSelect(type, extraData, commandId)}
        />
      </div>
    </div>
  );
};

// ── Export helper ────────────────────────────────────────────────────────────

async function exportCapsuleMarkdown(capsule: CapsuleNode) {
  const md = vaultStore.exportCapsuleToMarkdown(capsule.messageId);
  if (!md) return;
  const safe = capsule.subject.replace(/[^a-zA-Z0-9а-яА-ЯЁё_-]+/g, '_').slice(0, 60) || 'capsule';
  const fileName = `${safe}.md`;
  if (isDesktop()) {
    const path = await pickSavePath(fileName);
    if (path) await writeTextFile(path, md);
  } else {
    const blob = new Blob([md], { type: 'text/markdown' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(url);
  }
}

// ── Related capsules (backlinks) panel ───────────────────────────────────────

const RelatedCapsules: React.FC<{
  capsule: CapsuleNode;
  onOpenCapsule?: (messageId: string) => void;
}> = ({ capsule, onOpenCapsule }) => {
  const { t } = useI18n();
  const outgoing = vaultStore.getBacklinks(capsule.messageId);
  const incoming = vaultStore.getIncomingBacklinks(capsule.messageId);
  // tf-idf cosine over the same inverted index — deterministic "similar by meaning".
  const similar = vaultStore.relatedCapsules(capsule.messageId, 3);
  if (outgoing.length === 0 && incoming.length === 0 && similar.length === 0) return null;

  const renderLink = (id: string, dir: 'out' | 'in') => {
    const node = vaultStore.getNode(id);
    if (!node) return null;
    return (
      <button
        key={id}
        type="button"
        onClick={() => onOpenCapsule?.(id)}
        className="group/link flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-slate-800/60 hover:bg-cyan-950/40 border border-slate-700/60 hover:border-cyan-500/40 text-left transition-all"
        title={id}
      >
        <span className={`text-[10px] font-mono ${dir === 'out' ? 'text-cyan-400' : 'text-purple-400'}`}>
          {dir === 'out' ? '→' : '←'}
        </span>
        <span className="text-xs text-slate-300 group-hover/link:text-cyan-200 truncate">
          {node.subject}
        </span>
        <span className="ml-auto text-[10px] font-mono text-slate-500 shrink-0">
          {node.blocks.length} blk
        </span>
      </button>
    );
  };

  return (
    <div className="rounded-2xl border border-slate-800 bg-[#0f172a]/30 shadow-xl overflow-hidden backdrop-blur-md">
      <div className="px-5 py-3 border-b border-slate-800/80 flex items-center gap-2">
        <Layers className="w-4 h-4 text-cyan-400" />
        <span className="text-xs font-semibold text-slate-300 uppercase tracking-wider">
          {t('editor.relatedCapsules')}
        </span>
        <span className="text-[10px] font-mono text-slate-500 ml-auto">
          {incoming.length} ← / → {outgoing.length}
          {similar.length > 0 ? t('editor.similarCount', { count: similar.length }) : ''}
        </span>
      </div>
      <div className="p-4 grid grid-cols-1 sm:grid-cols-2 gap-2">
        <div className="space-y-1.5">
          <div className="text-[10px] font-mono text-purple-400/80 uppercase tracking-wider mb-1">
            {t('editor.incoming')}
          </div>
          {incoming.length > 0 ? incoming.map((id) => renderLink(id, 'in')) : (
            <div className="text-[11px] text-slate-600">{t('editor.incomingEmpty')}</div>
          )}
        </div>
        <div className="space-y-1.5">
          <div className="text-[10px] font-mono text-cyan-400/80 uppercase tracking-wider mb-1">
            {t('editor.outgoing')}
          </div>
          {outgoing.length > 0 ? outgoing.map((id) => renderLink(id, 'out')) : (
            <div className="text-[11px] text-slate-600">{t('editor.outgoingEmpty')}</div>
          )}
        </div>
      </div>
      {similar.length > 0 && (
        <div className="px-4 pb-4">
          <div className="text-[10px] font-mono text-emerald-400/80 uppercase tracking-wider mb-1.5">
            {t('editor.relatedTfidf')}
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {similar.map(({ node, similarity }) => (
              <button
                key={node.messageId}
                type="button"
                onClick={() => onOpenCapsule?.(node.messageId)}
                className="group/link flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-emerald-950/20 hover:bg-emerald-950/40 border border-emerald-900/40 hover:border-emerald-500/40 text-left transition-all"
                title={t('editor.relatedTitle', { score: (similarity * 100).toFixed(1) })}
              >
                <span className="text-[10px] font-mono text-emerald-400">≈</span>
                <span className="text-xs text-slate-300 group-hover/link:text-emerald-200 truncate">
                  {node.subject}
                </span>
                <span className="ml-auto text-[10px] font-mono text-emerald-400/90 shrink-0 tabular-nums">
                  {Math.round(similarity * 100)}%
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};
