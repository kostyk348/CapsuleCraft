import React, { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import JSZip from 'jszip';
import { CapsuleNode, CapsuleBlock, VaultGraphState, TodoItem, TodoPriority } from './types';
import { vaultStore } from './engine/vaultStore';
import { offlineAiResponse } from './engine/offlineAi';
import { assembleEml, parseEml } from './engine/mime';
import { UndoHistory } from './engine/history';
import { wouldCreatePageCycle } from './engine/hierarchy';
import { Navbar, ActiveView } from './components/Navbar';
import { VaultSidebar } from './components/VaultSidebar';
import { BlockCanvasEditor } from './components/BlockCanvasEditor';
import { GraphView } from './components/GraphView';
import { ThoughtThreadView } from './components/ThoughtThreadView';
import { RawMimeInspector } from './components/RawMimeInspector';
import { AutonomousHtmlPreview } from './components/AutonomousHtmlPreview';
import { BenchmarkModal } from './components/BenchmarkModal';
import { AiCoThinkModal } from './components/AiCoThinkModal';
import { EmptyVaultState, FaqModal } from './components/FaqOnboarding';
import { McpModal } from './components/McpModal';
import { CommandPalette, PaletteAction } from './components/CommandPalette';
import { PlanningView } from './components/PlanningView';
import { useI18n } from './i18n';

/**
 * Insert an AI-generated block, replacing the previous one in place when the
 * matcher says it is the same artefact (same persona's critique / same
 * synthesis callout). Re-running Co-Think must refresh, not stack duplicates.
 */
function upsertBlock(
  blocks: CapsuleBlock[],
  candidate: CapsuleBlock,
  matcher: (block: CapsuleBlock) => boolean,
): { blocks: CapsuleBlock[]; replaced: boolean } {
  const index = blocks.map((b, i) => (matcher(b) ? i : -1)).filter((i) => i >= 0).pop();
  if (index === undefined) return { blocks: [...blocks, candidate], replaced: false };
  const next = [...blocks];
  next[index] = candidate;
  return { blocks: next, replaced: true };
}

export default function App() {  const { t } = useI18n();
  const [graphState, setGraphState] = useState<VaultGraphState>(() => vaultStore.getState());
  const [selectedMessageId, setSelectedMessageId] = useState<string>('msg-001@capsuleos.local');
  const [activeView, setActiveView] = useState<ActiveView>('canvas');
  const [isBenchmarkOpen, setIsBenchmarkOpen] = useState(false);
  const [isAiModalOpen, setIsAiModalOpen] = useState(false);
  const [isMcpOpen, setIsMcpOpen] = useState(false);
  const [isFaqOpen, setIsFaqOpen] = useState(false);
  const [isPaletteOpen, setIsPaletteOpen] = useState(false);
  const [isAiLoading, setIsAiLoading] = useState(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  // Per-capsule undo history (serialized snapshots). Kept in a ref so typing
  // does not re-render the app; `editTick` only bumps the palette's canUndo.
  const histories = useRef<Map<string, UndoHistory>>(new Map());
  const lastEditAt = useRef<Map<string, number>>(new Map());
  const [editTick, setEditTick] = useState(0);

  const historyFor = (messageId: string): UndoHistory => {
    let h = histories.current.get(messageId);
    if (!h) {
      h = new UndoHistory();
      histories.current.set(messageId, h);
    }
    return h;
  };

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(() => setToastMessage(null), 3000);
  };

  // Subscribe to VaultStore changes
  useEffect(() => {
    const unsubscribe = vaultStore.subscribe(() => {
      setGraphState(vaultStore.getState());
    });
    return () => unsubscribe();
  }, []);

  const currentCapsule = selectedMessageId
    ? graphState.nodes.get(selectedMessageId) || Array.from(graphState.nodes.values())[0]
    : Array.from(graphState.nodes.values())[0];

  // Fresh installs start with an empty vault — the FAQ/onboarding page takes
  // the stage until the first capsule exists. No demo seed exists.
  const isVaultEmpty = graphState.nodes.size === 0;

  // Update capsule in memory (with undo history)
  const handleUpdateCapsule = useCallback((updated: CapsuleNode) => {
    const previous = vaultStore.getNode(updated.messageId);
    if (previous) {
      const snap = JSON.stringify(previous);
      const now = Date.now();
      const last = lastEditAt.current.get(updated.messageId) || 0;
      // Coalesce continuous typing: one undo step per ~700ms burst.
      if (now - last > 700) historyFor(updated.messageId).push(snap);
      lastEditAt.current.set(updated.messageId, now);
    }
    vaultStore.saveNode(updated);
    setEditTick((t) => t + 1);
  }, []);

  // ── Planning: task mutations across capsule todo blocks ─────────────────────
  const mutateTask = useCallback(
    (
      capsuleId: string,
      blockId: string,
      itemId: string,
      updater: (item: TodoItem) => TodoItem,
    ) => {
      const capsule = vaultStore.getNode(capsuleId);
      if (!capsule) return;
      const blocks = capsule.blocks.map((block) =>
        block.type === 'todo_list' && block.id === blockId
          ? {
              ...block,
              data: {
                ...block.data,
                items: (block.data.items || []).map((item) =>
                  item.id === itemId ? updater(item) : item,
                ),
              },
            }
          : block,
      );
      handleUpdateCapsule({ ...capsule, blocks });
    },
    [handleUpdateCapsule],
  );

  const handleToggleTask = useCallback(
    (capsuleId: string, blockId: string, itemId: string) => {
      mutateTask(capsuleId, blockId, itemId, (item) => {
        const done = !item.done;
        return { ...item, done, status: done ? 'done' : 'todo' };
      });
    },
    [mutateTask],
  );

  const handleUpdateTask = useCallback(
    (capsuleId: string, blockId: string, itemId: string, patch: Partial<TodoItem>) => {
      mutateTask(capsuleId, blockId, itemId, (item) => ({ ...item, ...patch }));
    },
    [mutateTask],
  );

  /** Quick add from the planning view: append to the inbox capsule's todo block. */
  const handleAddTask = useCallback(
    (text: string, due?: string, priority: TodoPriority = 'normal') => {
      const item: TodoItem = {
        id: `todo_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 5)}`,
        text,
        done: false,
        status: 'todo',
        due,
        priority,
      };
      let target = vaultStore.getNodes().find((node) => node.tags.includes('#inbox'));
      if (!target) target = vaultStore.getNodes()[0];
      if (!target) {
        target = vaultStore.createNewCapsule({
          subject: t('app.inboxSubject'),
          tags: ['#inbox'],
          blocks: [
            {
              id: `blk_todo_${Date.now()}`,
              type: 'todo_list',
              data: { title: t('app.inboxSubject'), items: [item] },
            },
          ],
        });
      } else {
        const todoBlock = target.blocks.find((block) => block.type === 'todo_list');
        const blocks: CapsuleBlock[] = todoBlock
          ? target.blocks.map((block) =>
              block.id === todoBlock.id && block.type === 'todo_list'
                ? { ...block, data: { ...block.data, items: [...(block.data.items || []), item] } }
                : block,
            )
          : [
              ...target.blocks,
              {
                id: `blk_todo_${Date.now()}`,
                type: 'todo_list',
                data: { title: t('app.inboxSubject'), items: [item] },
              },
            ];
        handleUpdateCapsule({ ...target, blocks });
      }
      showToast(due ? t('planning.toastAddedDue', { due }) : t('planning.toastAdded'));
    },
    [handleUpdateCapsule, t],
  );

  const undo = useCallback(() => {
    const id = selectedMessageId;
    const current = id ? vaultStore.getNode(id) : undefined;
    if (!id || !current) return;
    const snapshot = historyFor(id).undo(JSON.stringify(current));
    if (snapshot === null) {
      showToast(t('app.toast.undoEmpty'));
      return;
    }
    vaultStore.saveNode(JSON.parse(snapshot));
    lastEditAt.current.delete(id);
    setEditTick((t) => t + 1);
    showToast(t('app.toast.undoDone'));
  }, [selectedMessageId, t]);

  const redo = useCallback(() => {
    const id = selectedMessageId;
    const current = id ? vaultStore.getNode(id) : undefined;
    if (!id || !current) return;
    const snapshot = historyFor(id).redo(JSON.stringify(current));
    if (snapshot === null) {
      showToast(t('app.toast.redoEmpty'));
      return;
    }
    vaultStore.saveNode(JSON.parse(snapshot));
    lastEditAt.current.delete(id);
    setEditTick((t) => t + 1);
    showToast(t('app.toast.redoDone'));
  }, [selectedMessageId, t]);

  // Global keyboard: ⌘K palette, ⌘Z / ⇧⌘Z undo & redo.
  // While typing in a field the browser's native text undo wins.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setIsPaletteOpen((open) => !open);
        return;
      }
      const target = e.target as HTMLElement | null;
      const typing =
        !!target &&
        (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable);
      if (!mod || typing) return;
      const key = e.key.toLowerCase();
      if (key === 'z') {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      } else if (key === 'y') {
        e.preventDefault();
        redo();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [undo, redo]);

  // Create new blank capsule
  const handleCreateNewCapsule = useCallback(() => {
    const newId = `msg-${Date.now().toString(36)}@capsuleos.local`;
    const newCapsule: CapsuleNode = {
      messageId: newId,
      subject: t('app.newCapsuleSubject'),
      from: 'Local user <user@capsuleos.local>',
      to: 'CapsuleCraft Vault <vault@capsuleos.local>',
      date: new Date().toUTCString(),
      timestamp: Date.now(),
      references: [],
      status: 'Active',
      tags: [],
      blocks: [
        {
          id: `blk_${Date.now()}_1`,
          type: 'paragraph',
          data: { text: '' },
        },
      ],
      attachments: [],
      fileName: `capsule_${Date.now()}.eml`,
    };

    vaultStore.saveNode(newCapsule);
    setSelectedMessageId(newId);
    setActiveView('canvas');
    showToast(t('app.toast.capsuleCreated'));
  }, [t]);

  // Create a wiki sub-page. Hierarchy uses X-Capsule-Page-Parent, not
  // In-Reply-To, so a page is not silently treated as a mail reply.
  const handleCreateChildCapsule = useCallback((parentId: string) => {
    const parent = graphState.nodes.get(parentId);
    const child = vaultStore.createNewCapsule({
      subject: t('app.newSubpageSubject', {
        parent: parent?.subject || t('editor.untitled'),
      }),
      parentId,
      tags: parent ? [...parent.tags] : [],
    });

    setSelectedMessageId(child.messageId);
    setActiveView('canvas');
    showToast(t('app.toast.subpageCreated', { subject: child.subject }));
  }, [graphState.nodes, t]);

  /** Move a page in the tree; cycles are rejected before the store is touched. */
  const handleReparentCapsule = useCallback((pageId: string, parentId: string | null) => {
    const node = vaultStore.getNode(pageId);
    if (!node) return;

    if (parentId && wouldCreatePageCycle(graphState.nodes, pageId, parentId)) {
      showToast(t('app.toast.parentCycle'));
      return;
    }
    if ((node.parentId || null) === (parentId || null)) return;

    handleUpdateCapsule({ ...node, parentId: parentId || undefined });
    showToast(t('app.toast.parentChanged'));
  }, [graphState.nodes, handleUpdateCapsule, t]);

  // Create In-Reply-To child capsule. `blocks` lets the caller inject
  // AI-generated content (offline Co-Thinker) instead of the default template.
  const handleCreateReply = useCallback((parentId: string, title?: string, blocks?: CapsuleNode['blocks']) => {
    const parent = graphState.nodes.get(parentId);
    const newId = `msg-reply-${Date.now().toString(36)}@capsuleos.local`;
    const finalTitle = title || (parent ? `Re: ${parent.subject}` : t('app.replyReflection'));
    const newCapsule: CapsuleNode = {
      messageId: newId,
      subject: finalTitle,
      from: 'Alexey <alexey@workspace.local>',
      to: 'CapsuleCraft Knowledge Vault <vault@capsuleos.local>',
      date: new Date().toUTCString(),
      timestamp: Date.now(),
      inReplyTo: parentId,
      references: parent ? [parentId, ...(parent.references || [])] : [parentId],
      status: 'Active',
      tags: parent ? [...parent.tags] : ['#dialogue'],
      blocks: blocks || [
        {
          id: `blk_${Date.now()}_1`,
          type: 'paragraph',
          data: { text: '' },
        },
      ],
      attachments: [],
      fileName: `reply_${Date.now()}.eml`,
    };

    vaultStore.saveNode(newCapsule);
    setSelectedMessageId(newId);
    setActiveView('canvas');
    showToast(t('app.toast.replyCreated', { subject: finalTitle }));
  }, [graphState.nodes, t]);

  // Delete Capsule
  const handleDeleteCapsule = useCallback((messageId: string) => {
    const wasSelected = messageId === selectedMessageId;
    vaultStore.deleteNode(messageId);
    // Only reset the selection when the deleted capsule was on screen.
    if (wasSelected) {
      const remaining = Array.from(vaultStore.getState().nodes.keys());
      if (remaining.length > 0) {
        setSelectedMessageId(remaining[0]);
      }
    }
    showToast(t('app.toast.removed'));
  }, [selectedMessageId, t]);

  // Import .eml file
  const handleImportEml = useCallback(async (file: File) => {
    try {
      const rawText = await file.text();
      const parsed = parseEml(rawText, file.name);
      vaultStore.saveNode(parsed);
      setSelectedMessageId(parsed.messageId);
      setActiveView('canvas');
      showToast(t('app.toast.importedFile', { name: file.name }));
    } catch (err: any) {
      showToast(t('app.toast.importError', { message: err.message }));
    }
  }, [t]);

  // Export single .eml
  const handleDownloadEml = useCallback((messageId: string) => {
    const node = graphState.nodes.get(messageId);
    if (!node) return;
    const rawEml = node.rawEml || assembleEml(node);
    const blob = new Blob([rawEml], { type: 'message/rfc822' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = node.fileName || `${node.subject.replace(/[^a-zA-Z0-9_-]/g, '_')}.eml`;
    a.click();
    URL.revokeObjectURL(url);
    showToast(t('app.toast.downloaded', { name: a.download }));
  }, [graphState.nodes, t]);

  // Export whole vault as ZIP
  const handleExportVaultZip = useCallback(async () => {
    const zip = new JSZip();
    const allNodes: CapsuleNode[] = Array.from(graphState.nodes.values());

    for (const node of allNodes) {
      const rawEml = node.rawEml || assembleEml(node);
      const filename = node.fileName || `${node.subject.replace(/[^a-zA-Z0-9_-]/g, '_')}.eml`;
      zip.file(filename, rawEml);
    }

    const zipBlob = await zip.generateAsync({ type: 'blob' });
    const url = URL.createObjectURL(zipBlob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `CapsuleCraft_Vault_${new Date().toISOString().slice(0, 10)}.zip`;
    a.click();
    URL.revokeObjectURL(url);
    showToast(t('app.toast.vaultExported', { count: allNodes.length }));
  }, [graphState.nodes, t]);

  // AI Co-Thinking trigger.
  // Accepts both the editor's short actions and the Co-Think modal modes:
  //   critique | counter_arg | synthesis | data_insight | continue | reply_thread
  const handleAiAction = useCallback(async (
    action: 'critique' | 'continue' | 'reply_thread' | 'data_insight' | string,
    customPrompt?: string,
  ) => {
    if (!currentCapsule) return;
    setIsAiLoading(true);
    const persona = 'Co-Thinker';

    // Normalize modal mode names onto engine actions.
    // counter_arg -> reply_thread (child capsule), synthesis -> continue (callout).
    const mode =
      action === 'synthesis' ? 'continue'
      : action === 'counter_arg' ? 'reply_thread'
      : action;

    try {
      const defaultPrompt = mode === 'critique'
        ? t('ai.defaultPrompt.critique', { subject: currentCapsule.subject })
        : mode === 'continue'
        ? t('ai.defaultPrompt.continue', { subject: currentCapsule.subject })
        : mode === 'reply_thread'
        ? t('ai.defaultPrompt.counter', { subject: currentCapsule.subject })
        : t('ai.defaultPrompt.data', { subject: currentCapsule.subject });

      const promptToSend = customPrompt || defaultPrompt;

      // Offline deterministic Co-Thinker — local heuristics, no server or model call.
      // The engine reads the REAL capsule content (blocks, claims, CSV attachments)
      // and gets the vault context for honest related-capsule suggestions.
      await new Promise((r) => setTimeout(r, 350)); // keep spinner visible for UX
      const data = offlineAiResponse(mode, currentCapsule, promptToSend, persona, {
        vaultNodes: Array.from(graphState.nodes.values()),
      });
      const aiReply =
        data.reply ||
        data.text ||
        data.result?.criticism ||
        data.result?.text ||
        (typeof data.result === 'string' ? data.result : '') ||
        t('app.aiDone');

      if (mode === 'reply_thread') {
        // Reuse the AI-generated blocks instead of the default template.
        const aiBlocks = data.result?.blocks;
        handleCreateReply(
          currentCapsule.messageId,
          data.result?.subject || `Re: ${currentCapsule.subject} (${persona})`,
          Array.isArray(aiBlocks) && aiBlocks.length > 0 ? aiBlocks : undefined
        );
      } else if (mode === 'continue') {
        const newCallout: CapsuleBlock = {
          id: `blk_ai_cont_${Date.now()}`,
          type: 'callout',
          data: {
            title: `AI Synthesis (${persona})`,
            content: aiReply,
            tone: 'emerald',
          },
        };
        const { blocks, replaced } = upsertBlock(
          currentCapsule.blocks,
          newCallout,
          (b) => b.type === 'callout' && String((b.data as any).title || '').startsWith('AI Synthesis'),
        );
        handleUpdateCapsule({ ...currentCapsule, blocks });
        showToast(replaced
          ? t('app.toast.aiUpdated', { label: 'AI Synthesis', persona })
          : t('app.toast.aiAdded', { label: 'AI Synthesis', persona }));
      } else if (mode === 'data_insight') {
        const newCallout: CapsuleBlock = {
          id: `blk_ai_data_${Date.now()}`,
          type: 'callout',
          data: {
            title: `Data Insight (${persona})`,
            content: aiReply,
            tone: 'blue',
          },
        };
        const { blocks, replaced } = upsertBlock(
          currentCapsule.blocks,
          newCallout,
          (b) => b.type === 'callout' && String((b.data as any).title || '').startsWith('Data Insight'),
        );
        handleUpdateCapsule({ ...currentCapsule, blocks });
        showToast(replaced
          ? t('app.toast.aiUpdated', { label: 'Data Insight', persona })
          : t('app.toast.aiAdded', { label: 'Data Insight', persona }));
      } else {
        // Insert (or refresh) the AI opponent block for this persona.
        const newBlock: CapsuleBlock = {
          id: `blk_ai_${Date.now()}`,
          type: 'ai_opponent_block',
          author: persona,
          data: {
            criticism: aiReply,
            confidence: data.result?.confidence ?? 0.94,
            sourceClaim: data.result?.sourceClaim || currentCapsule.subject,
          },
        };
        const { blocks, replaced } = upsertBlock(
          currentCapsule.blocks,
          newBlock,
          (b) => b.type === 'ai_opponent_block' && (b.author || '') === persona,
        );
        handleUpdateCapsule({ ...currentCapsule, blocks });
        showToast(replaced
          ? t('app.toast.aiUpdated', { label: 'AI Critique', persona })
          : t('app.toast.aiAdded', { label: 'AI Critique', persona }));
      }
    } catch (err: any) {
      console.warn('AI API error, using client fallback:', err);
      const fallbackBlock = {
        id: `blk_ai_${Date.now()}`,
        type: 'ai_opponent_block' as const,
        author: persona,
        data: {
          criticism: t('app.aiFallbackCriticism', { persona, subject: currentCapsule.subject }),
          confidence: 0.92,
          sourceClaim: currentCapsule.subject,
        },
      };
      handleUpdateCapsule({
        ...currentCapsule,
        blocks: [...currentCapsule.blocks, fallbackBlock],
      });
      showToast(t('app.toast.aiFallback'));
    } finally {
      setIsAiLoading(false);
      setIsAiModalOpen(false);
    }
  }, [currentCapsule, graphState.nodes, handleCreateReply, handleUpdateCapsule, t]);

  // Command palette entries — navigation, actions and (from the component
  // itself) BM25-ranked capsules. `editTick` keeps undo/redo state fresh.
  const paletteActions = useMemo<PaletteAction[]>(() => {
    const id = selectedMessageId;
    const hist = id ? histories.current.get(id) : undefined;
    return [
      {
        id: 'new-capsule',
        title: t('palette.action.new.title'),
        hint: t('palette.action.new.hint'),
        group: t('palette.group.actions'),
        keywords: t('palette.action.new.keywords').split(' '),
        run: handleCreateNewCapsule,
      },
      {
        id: 'undo',
        title: t('palette.action.undo.title'),
        hint: hist?.canUndo
          ? t('palette.action.undo.hintAvailable', { count: hist.depth })
          : t('palette.action.undo.hintEmpty'),
        group: t('palette.group.actions'),
        keywords: t('palette.action.undo.keywords').split(' '),
        shortcut: '⌘Z',
        run: undo,
      },
      {
        id: 'redo',
        title: t('palette.action.redo.title'),
        hint: hist?.canRedo ? t('palette.action.redo.hintAvailable') : t('palette.action.redo.hintEmpty'),
        group: t('palette.group.actions'),
        keywords: t('palette.action.redo.keywords').split(' '),
        shortcut: '⇧⌘Z',
        run: redo,
      },
      {
        id: 'export-vault',
        title: t('palette.action.export.title'),
        hint: t('palette.action.export.hint'),
        group: t('palette.group.actions'),
        keywords: t('palette.action.export.keywords').split(' '),
        run: handleExportVaultZip,
      },
      {
        id: 'benchmark',
        title: t('palette.action.bench.title'),
        hint: t('palette.action.bench.hint'),
        group: t('palette.group.tools'),
        keywords: t('palette.action.bench.keywords').split(' '),
        run: () => setIsBenchmarkOpen(true),
      },
      {
        id: 'cothink',
        title: t('palette.action.cothink.title'),
        hint: t('palette.action.cothink.hint'),
        group: t('palette.group.tools'),
        keywords: t('palette.action.cothink.keywords').split(' '),
        run: () => setIsAiModalOpen(true),
      },
      {
        id: 'connect-mcp',
        title: t('mcp.title'),
        hint: 'Model Context Protocol (Claude, Antigravity, Cursor)',
        group: t('palette.group.tools'),
        keywords: t('palette.action.mcp.keywords').split(' '),
        run: () => setIsMcpOpen(true),
      },
      {
        id: 'faq',
        title: t('palette.action.faq.title'),
        hint: t('palette.action.faq.hint'),
        group: t('palette.group.tools'),
        keywords: t('palette.action.faq.keywords').split(' '),
        run: () => setIsFaqOpen(true),
      },
      { id: 'view-editor', title: t('palette.action.view.editor'), group: t('palette.group.navigation'), keywords: t('palette.action.view.editor.keywords').split(' '), run: () => setActiveView('canvas') },
      { id: 'view-graph', title: t('palette.action.view.graph'), group: t('palette.group.navigation'), keywords: t('palette.action.view.graph.keywords').split(' '), run: () => setActiveView('graph') },
      { id: 'view-thread', title: t('palette.action.view.thread'), group: t('palette.group.navigation'), keywords: t('palette.action.view.thread.keywords').split(' '), run: () => setActiveView('thread') },
      { id: 'view-planning', title: t('palette.action.view.planning'), group: t('palette.group.navigation'), keywords: t('palette.action.view.planning.keywords').split(' '), run: () => setActiveView('planning') },
      { id: 'view-mime', title: t('palette.action.view.mime'), group: t('palette.group.navigation'), keywords: t('palette.action.view.mime.keywords').split(' '), run: () => setActiveView('mime') },
      { id: 'view-preview', title: t('palette.action.view.preview'), group: t('palette.group.navigation'), keywords: t('palette.action.view.preview.keywords').split(' '), run: () => setActiveView('html_preview') },
    ];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedMessageId, editTick, handleCreateNewCapsule, handleExportVaultZip, undo, redo, t]);

  return (
    <div className="w-screen h-screen flex flex-col bg-slate-950 text-slate-100 overflow-hidden font-sans select-none">
      {/* Top Navbar */}
      <Navbar
        activeView={activeView}
        onChangeView={setActiveView}
        onOpenBenchmark={() => setIsBenchmarkOpen(true)}
        onOpenAiCoThink={() => setIsAiModalOpen(true)}
        onOpenFaq={() => setIsFaqOpen(true)}
        onOpenMcp={() => setIsMcpOpen(true)}
        totalCapsules={graphState.graphNodes.length}
        scanDurationMs={graphState.scanStats.scanDurationMs}
      />

      {/* Main Workspace Frame */}
      <div className="flex-1 flex overflow-hidden relative">
        {/* Left Vault File Tree Sidebar */}
        <VaultSidebar
          graphState={graphState}
          selectedMessageId={selectedMessageId}
          onSelectNode={(id) => {
            setSelectedMessageId(id);
          }}
          onCreateNewCapsule={handleCreateNewCapsule}
          onCreateChildCapsule={handleCreateChildCapsule}
          onDeleteCapsule={handleDeleteCapsule}
          onImportEml={handleImportEml}
          onExportVaultZip={handleExportVaultZip}
        />

        {/* Center Main Stage View */}
        <main className="flex-1 h-full overflow-hidden bg-slate-950 flex flex-col">
          {isVaultEmpty && (
            <EmptyVaultState
              onCreateNew={handleCreateNewCapsule}
              onImportEml={handleImportEml}
            />
          )}

          {!isVaultEmpty && activeView === 'canvas' && currentCapsule && (
            <BlockCanvasEditor
              capsule={currentCapsule}
              allCapsules={Array.from(graphState.nodes.values())}
              onUpdateCapsule={handleUpdateCapsule}
              onOpenParent={(parentId) => {
                setSelectedMessageId(parentId);
              }}
              onReparent={(parentId) =>
                handleReparentCapsule(currentCapsule.messageId, parentId)
              }
              onCreateChild={() => handleCreateChildCapsule(currentCapsule.messageId)}
              onRequestAiAction={handleAiAction}
              isAiLoading={isAiLoading}
              onOpenCapsule={(id) => {
                if (id) setSelectedMessageId(id);
              }}
            />
          )}

          {!isVaultEmpty && activeView === 'graph' && (
            <GraphView
              graphState={graphState}
              selectedMessageId={selectedMessageId}
              onSelectNode={(id) => {
                if (id) setSelectedMessageId(id);
              }}
              onOpenInCanvas={(id) => {
                setSelectedMessageId(id);
                setActiveView('canvas');
              }}
            />
          )}

          {!isVaultEmpty && activeView === 'planning' && (
            <PlanningView
              nodes={Array.from(graphState.nodes.values())}
              onOpenCapsule={(id) => {
                setSelectedMessageId(id);
                setActiveView('canvas');
              }}
              onToggleTask={handleToggleTask}
              onUpdateTask={handleUpdateTask}
              onAddTask={handleAddTask}
            />
          )}

          {!isVaultEmpty && activeView === 'thread' && (
            <ThoughtThreadView
              graphState={graphState}
              selectedMessageId={selectedMessageId}
              onSelectNode={setSelectedMessageId}
              onOpenInCanvas={(id) => {
                setSelectedMessageId(id);
                setActiveView('canvas');
              }}
              onCreateReply={handleCreateReply}
            />
          )}

          {!isVaultEmpty && activeView === 'mime' && currentCapsule && (
            <RawMimeInspector
              capsule={currentCapsule}
              onDownloadEml={handleDownloadEml}
            />
          )}

          {!isVaultEmpty && activeView === 'html_preview' && currentCapsule && (
            <AutonomousHtmlPreview
              capsule={currentCapsule}
              onDownloadEml={handleDownloadEml}
            />
          )}
        </main>
      </div>

      {/* Toast Notification */}
      {toastMessage && (
        <div className="fixed bottom-6 right-6 z-50 bg-slate-900 border border-sky-500/50 text-slate-100 text-xs font-mono px-4 py-2.5 rounded-xl shadow-2xl animate-in fade-in slide-in-from-bottom-3 flex items-center gap-2">
          <span className="w-2 h-2 rounded-full bg-sky-400 animate-pulse" />
          <span>{toastMessage}</span>
        </div>
      )}

      {/* Benchmark Simulator Modal */}
      <BenchmarkModal
        isOpen={isBenchmarkOpen}
        onClose={() => setIsBenchmarkOpen(false)}
      />

      {/* FAQ / Help Modal */}
      <FaqModal isOpen={isFaqOpen} onClose={() => setIsFaqOpen(false)} />

      {/* Connect AI Agent (MCP) Modal */}
      <McpModal isOpen={isMcpOpen} onClose={() => setIsMcpOpen(false)} />

      {/* ⌘K command palette — search, navigation and actions */}
      <CommandPalette
        isOpen={isPaletteOpen}
        onClose={() => setIsPaletteOpen(false)}
        actions={paletteActions}
        onOpenCapsule={(id) => {
          setSelectedMessageId(id);
          setActiveView('canvas');
        }}
        onImportEml={handleImportEml}
      />

      {/* AI Co-Think Modal */}
      {currentCapsule && (
        <AiCoThinkModal
          isOpen={isAiModalOpen}
          onClose={() => setIsAiModalOpen(false)}
          currentCapsule={currentCapsule}
          isLoading={isAiLoading}
          onExecuteAi={(prompt, mode) => {
            handleAiAction(mode, prompt);
          }}
        />
      )}
    </div>
  );
}
