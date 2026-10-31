import React, { useState, useRef, useEffect, useMemo } from 'react';
import {
  CapsuleBlock,
  InteractiveCanvasBlockData,
  CapsuleAttachment,
  WhiteboardElement,
} from '../../types';
import {
  Square,
  StickyNote,
  ArrowRight,
  ZoomIn,
  ZoomOut,
  Maximize2,
  Trash2,
  Move,
  Type,
  Plus,
  Palette,
  Sparkles,
} from 'lucide-react';
import { useI18n } from '../../i18n';

interface Props {
  block: CapsuleBlock & { type: 'interactive_canvas' };
  attachments: CapsuleAttachment[];
  onChange: (updatedData: InteractiveCanvasBlockData) => void;
  onUpdateAttachment: (cid: string, newContent: string) => void;
  onDelete: () => void;
}

export const WhiteboardBlock: React.FC<Props> = ({
  block,
  attachments,
  onChange,
  onUpdateAttachment,
  onDelete,
}) => {
  const { t } = useI18n();
  const cleanCid = block.data.scene_cid.replace(/^cid:/, '').replace(/^<|>$/g, '');
  const attachment = attachments.find(
    (a) => a.cid === cleanCid || `cid:${a.cid}` === block.data.scene_cid || a.name === cleanCid
  );

  const initialElements: WhiteboardElement[] = useMemo(() => {
    if (attachment && attachment.content) {
      try {
        const parsed = JSON.parse(attachment.content);
        if (parsed.elements && Array.isArray(parsed.elements)) {
          return parsed.elements;
        }
      } catch (e) {
        console.warn('Could not parse whiteboard attachment:', e);
      }
    }
    return [];
  }, [attachment]);

  const [elements, setElements] = useState<WhiteboardElement[]>(initialElements);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dragOffset, setDragOffset] = useState({ x: 0, y: 0 });
  const canvasRef = useRef<HTMLDivElement>(null);

  // Sync back to attachment when elements change
  const syncElements = (updated: WhiteboardElement[]) => {
    setElements(updated);
    const sceneJson = JSON.stringify({ elements: updated }, null, 2);
    onUpdateAttachment(cleanCid, sceneJson);
  };

  const handleMouseDown = (e: React.MouseEvent, el: WhiteboardElement) => {
    e.stopPropagation();
    setSelectedId(el.id);
    setDraggingId(el.id);
    const rect = canvasRef.current?.getBoundingClientRect();
    if (rect) {
      const mouseX = (e.clientX - rect.left) / zoom;
      const mouseY = (e.clientY - rect.top) / zoom;
      setDragOffset({
        x: mouseX - el.x,
        y: mouseY - el.y,
      });
    }
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!draggingId) return;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (rect) {
      const mouseX = (e.clientX - rect.left) / zoom;
      const mouseY = (e.clientY - rect.top) / zoom;

      const newX = Math.round(Math.max(10, mouseX - dragOffset.x));
      const newY = Math.round(Math.max(10, mouseY - dragOffset.y));

      const updated = elements.map((el) =>
        el.id === draggingId ? { ...el, x: newX, y: newY } : el
      );
      setElements(updated);
    }
  };

  const handleMouseUp = () => {
    if (draggingId) {
      setDraggingId(null);
      syncElements(elements);
    }
  };

  const handleAddElement = (type: WhiteboardElement['type']) => {
    const id = `el_${Date.now().toString(36)}`;
    const newEl: WhiteboardElement = {
      id,
      type,
      x: 100 + elements.length * 30,
      y: 80 + (elements.length % 3) * 40,
      width: type === 'sticky' ? 200 : type === 'arrow' ? 120 : 180,
      height: type === 'sticky' ? 85 : type === 'arrow' ? 4 : 65,
      label: type === 'sticky' ? t('board.stickyLabel') : type === 'arrow' ? 'Link' : 'Mind Node',
      color: type === 'sticky' ? '#f59e0b' : '#38bdf8',
    };
    const updated = [...elements, newEl];
    syncElements(updated);
    setSelectedId(id);
  };

  const handleUpdateLabel = (id: string, label: string) => {
    const updated = elements.map((el) => (el.id === id ? { ...el, label } : el));
    syncElements(updated);
  };

  const handleChangeColor = (id: string, color: string) => {
    const updated = elements.map((el) => (el.id === id ? { ...el, color } : el));
    syncElements(updated);
  };

  const handleDeleteElement = (id: string) => {
    const updated = elements.filter((el) => el.id !== id);
    syncElements(updated);
    if (selectedId === id) setSelectedId(null);
  };

  return (
    <div className="my-4 rounded-xl border border-cyan-500/30 bg-[#0f172a]/80 shadow-xl shadow-cyan-950/20 backdrop-blur-sm overflow-hidden group">
      {/* Header Bar */}
      <div className="bg-slate-950/80 px-4 py-2.5 border-b border-slate-800 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <div className="p-1 rounded bg-cyan-500/10 text-cyan-400 border border-cyan-500/30">
            <Sparkles className="w-4 h-4" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-wider text-cyan-400">
                Interactive Whiteboard
              </span>
              <span className="text-[10px] font-mono text-slate-400 bg-slate-800/80 px-2 py-0.5 rounded border border-slate-700">
                {block.data.scene_cid}
              </span>
            </div>
          </div>
        </div>

        {/* Toolbar & Controls */}
        <div className="flex items-center gap-2">
          <div className="flex items-center bg-slate-800/80 rounded-lg p-0.5 border border-slate-700 text-xs">
            <button
              type="button"
              onClick={() => handleAddElement('rectangle')}
              className="flex items-center gap-1 px-2 py-1 rounded hover:bg-cyan-500/20 text-slate-300 hover:text-cyan-300 transition-colors"
              title="Add Node"
            >
              <Square className="w-3.5 h-3.5" />
              <span>Node</span>
            </button>
            <button
              type="button"
              onClick={() => handleAddElement('sticky')}
              className="flex items-center gap-1 px-2 py-1 rounded hover:bg-amber-500/20 text-slate-300 hover:text-amber-300 transition-colors"
              title="Add Sticky Note"
            >
              <StickyNote className="w-3.5 h-3.5" />
              <span>Sticky</span>
            </button>
            <button
              type="button"
              onClick={() => handleAddElement('arrow')}
              className="flex items-center gap-1 px-2 py-1 rounded hover:bg-cyan-500/20 text-slate-300 hover:text-cyan-300 transition-colors"
              title="Add Arrow Link"
            >
              <ArrowRight className="w-3.5 h-3.5" />
              <span>Arrow</span>
            </button>
          </div>

          {/* Zoom */}
          <div className="flex items-center bg-slate-800/80 rounded-lg p-0.5 border border-slate-700 text-xs">
            <button
              type="button"
              onClick={() => setZoom((z) => Math.max(0.6, z - 0.1))}
              className="p-1 text-slate-400 hover:text-slate-200"
              title="Zoom Out"
            >
              <ZoomOut className="w-3.5 h-3.5" />
            </button>
            <span className="px-1.5 text-[10px] font-mono text-slate-300">{Math.round(zoom * 100)}%</span>
            <button
              type="button"
              onClick={() => setZoom((z) => Math.min(1.5, z + 0.1))}
              className="p-1 text-slate-400 hover:text-slate-200"
              title="Zoom In"
            >
              <ZoomIn className="w-3.5 h-3.5" />
            </button>
          </div>

          <button
            type="button"
            onClick={onDelete}
            className="text-xs text-slate-500 hover:text-rose-400 p-1.5 rounded hover:bg-slate-800 transition-colors"
            title="Remove Whiteboard"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Canvas Area with Dot Grid */}
      <div
        ref={canvasRef}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onClick={() => setSelectedId(null)}
        style={{
          backgroundImage: `radial-gradient(rgba(148, 163, 184, 0.15) 1px, transparent 1px)`,
          backgroundSize: '24px 24px',
        }}
        className="relative h-[320px] w-full overflow-hidden bg-[#020617] select-none cursor-crosshair"
      >
        <div
          style={{
            transform: `scale(${zoom})`,
            transformOrigin: '0 0',
          }}
          className="relative w-full h-full"
        >
          {elements.length === 0 && (
            <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
              <p className="text-[11px] font-mono text-slate-600">{t('board.empty')}</p>
            </div>
          )}
          {/* Render Arrow Connectors */}
          {elements
            .filter((el) => el.type === 'arrow')
            .map((el) => (
              <div
                key={el.id}
                style={{
                  left: `${el.x}px`,
                  top: `${el.y}px`,
                  width: `${el.width}px`,
                }}
                onClick={(e) => {
                  e.stopPropagation();
                  setSelectedId(el.id);
                }}
                onMouseDown={(e) => handleMouseDown(e, el)}
                className={`absolute flex items-center cursor-move group/arrow ${
                  selectedId === el.id ? 'ring-1 ring-cyan-400' : ''
                }`}
              >
                <div className="h-0.5 flex-1 bg-slate-600 group-hover/arrow:bg-cyan-400 transition-colors" />
                <ArrowRight className="w-4 h-4 text-slate-500 group-hover/arrow:text-cyan-400 shrink-0 -ml-1" />
                {el.label && (
                  <span className="absolute -top-5 left-1/2 -translate-x-1/2 text-[10px] font-mono text-slate-400 bg-slate-900 px-1.5 py-0.5 rounded border border-slate-800">
                    {el.label}
                  </span>
                )}
              </div>
            ))}

          {/* Render Rectangle / Nodes & Stickies */}
          {elements
            .filter((el) => el.type !== 'arrow')
            .map((el) => {
              const isSelected = selectedId === el.id;
              const isSticky = el.type === 'sticky';

              return (
                <div
                  key={el.id}
                  style={{
                    left: `${el.x}px`,
                    top: `${el.y}px`,
                    width: `${el.width}px`,
                    minHeight: `${el.height}px`,
                    borderColor: el.color,
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    setSelectedId(el.id);
                  }}
                  onMouseDown={(e) => handleMouseDown(e, el)}
                  className={`absolute rounded-xl p-3 shadow-md transition-shadow cursor-grab active:cursor-grabbing backdrop-blur-md ${
                    isSticky
                      ? 'bg-amber-950/80 text-amber-100 border border-amber-500/50 shadow-amber-950/40'
                      : 'bg-[#0f172a]/95 text-slate-100 border border-slate-700 shadow-black/60'
                  } ${
                    isSelected
                      ? 'ring-2 ring-cyan-400 ring-offset-2 ring-offset-slate-950 shadow-lg'
                      : 'hover:border-slate-500'
                  }`}
                >
                  {/* Top bar of element */}
                  <div className="flex items-center justify-between mb-1.5 opacity-80">
                    <div className="flex items-center gap-1.5">
                      <span
                        className="w-2 h-2 rounded-full"
                        style={{ backgroundColor: el.color }}
                      />
                      <span className="text-[10px] uppercase font-mono tracking-wider text-slate-400">
                        {isSticky ? 'Sticky Note' : 'Mind Block'}
                      </span>
                    </div>

                    {isSelected && (
                      <div className="flex items-center gap-1">
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteElement(el.id);
                          }}
                          className="p-0.5 text-slate-400 hover:text-rose-400"
                          title="Delete element"
                        >
                          <Trash2 className="w-3 h-3" />
                        </button>
                      </div>
                    )}
                  </div>

                  {/* Text Input */}
                  <textarea
                    rows={2}
                    value={el.label}
                    onChange={(e) => handleUpdateLabel(el.id, e.target.value)}
                    className="w-full bg-transparent text-xs leading-snug resize-none border-none outline-none focus:ring-0 font-medium"
                    placeholder="Enter concept..."
                  />

                  {/* Color Selector on selection */}
                  {isSelected && (
                    <div className="flex items-center gap-1 mt-2 pt-1.5 border-t border-slate-700/50">
                      {['#06b6d4', '#10b981', '#a855f7', '#f59e0b', '#ec4899'].map((c) => (
                        <button
                          key={c}
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleChangeColor(el.id, c);
                          }}
                          style={{ backgroundColor: c }}
                          className={`w-3 h-3 rounded-full transition-transform ${
                            el.color === c ? 'scale-125 ring-1 ring-white' : 'opacity-70 hover:opacity-100'
                          }`}
                        />
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
        </div>

        {/* Footer Hint */}
        <div className="absolute bottom-2 right-3 text-[10px] font-mono text-slate-500 bg-slate-950/80 px-2 py-1 rounded border border-slate-800">
          {elements.length} elements • Drag canvas nodes freely
        </div>
      </div>
    </div>
  );
};
