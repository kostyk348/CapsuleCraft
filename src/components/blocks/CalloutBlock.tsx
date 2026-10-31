import React, { useState } from 'react';
import { CapsuleBlock, CalloutBlockData } from '../../types';
import { vaultStore } from '../../engine/vaultStore';
import { renderMarkdownMultiLine } from '../../engine/markdown';
import { Lightbulb, Info, AlertTriangle, Sparkles, Heart, Trash2, Eye, Pencil } from 'lucide-react';
import { useI18n } from '../../i18n';

interface Props {
  block: CapsuleBlock & { type: 'callout' };
  onChange: (updatedData: CalloutBlockData) => void;
  onDelete: () => void;
  onOpenCapsule?: (messageId: string) => void;
}

export const CalloutBlock: React.FC<Props> = ({ block, onChange, onDelete, onOpenCapsule }) => {
  const [showPreview, setShowPreview] = useState(false);
  const { t } = useI18n();
  const tone = block.data.tone || 'emerald';

  const DEFAULT_TONE = {
    border: 'border-cyan-500/40',
    bg: 'bg-cyan-950/20',
    text: 'text-cyan-300',
    icon: Lightbulb,
  };

  const toneConfig = ({
    emerald: {
      border: 'border-cyan-500/40',
      bg: 'bg-cyan-950/20',
      text: 'text-cyan-300',
      icon: Lightbulb,
    },
    blue: {
      border: 'border-sky-500/40',
      bg: 'bg-sky-950/20',
      text: 'text-sky-300',
      icon: Info,
    },
    purple: {
      border: 'border-purple-500/40',
      bg: 'bg-purple-950/20',
      text: 'text-purple-300',
      icon: Sparkles,
    },
    amber: {
      border: 'border-amber-500/40',
      bg: 'bg-amber-950/20',
      text: 'text-amber-300',
      icon: AlertTriangle,
    },
    rose: {
      border: 'border-rose-500/40',
      bg: 'bg-rose-950/20',
      text: 'text-rose-300',
      icon: Heart,
    },
    indigo: {
      border: 'border-indigo-500/40',
      bg: 'bg-indigo-950/20',
      text: 'text-indigo-300',
      icon: Info,
    },
  } as Record<string, typeof DEFAULT_TONE>)[tone] || DEFAULT_TONE;

  const IconComponent = toneConfig.icon;

  return (
    <div
      className={`my-3 rounded-xl border ${toneConfig.border} ${toneConfig.bg} p-4 shadow-md backdrop-blur-sm group relative`}
    >
      <div className="flex items-start gap-3">
        <div className={`p-1 rounded-lg ${toneConfig.text} shrink-0 mt-0.5`}>
          <IconComponent className="w-5 h-5" />
        </div>

        <div className="flex-1 space-y-1">
          <input
            type="text"
            value={block.data.title}
            onChange={(e) => onChange({ ...block.data, title: e.target.value })}
            placeholder="Callout title..."
            className={`w-full bg-transparent font-semibold text-sm outline-none border-none focus:ring-0 ${toneConfig.text}`}
          />

          {showPreview ? (
            <div
              className="w-full text-slate-300 text-xs leading-relaxed cursor-text"
              onDoubleClick={() => setShowPreview(false)}
              title={t('block.doubleClickEdit')}
            >
              {renderMarkdownMultiLine(
                block.data.content,
                (q) => vaultStore.resolveBacklink(q),
                onOpenCapsule
              )}
            </div>
          ) : (
            <textarea
              rows={2}
              value={block.data.content}
              onChange={(e) => onChange({ ...block.data, content: e.target.value })}
              placeholder={t('block.calloutPlaceholder')}
              className="w-full bg-transparent text-slate-300 text-xs leading-relaxed resize-none outline-none border-none focus:ring-0 placeholder:text-slate-500"
            />
          )}
        </div>

        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            type="button"
            onClick={() => setShowPreview((v) => !v)}
            className="p-1 rounded text-slate-500 hover:text-cyan-300 transition"
            title={showPreview ? t('block.edit') : t('block.preview')}
          >
            {showPreview ? <Pencil className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="p-1 rounded text-slate-500 hover:text-rose-400 transition"
            title="Delete Callout"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>
  );
};
