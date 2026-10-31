import React, { useState } from 'react';
import { CapsuleBlock, HeadingBlockData } from '../../types';
import { vaultStore } from '../../engine/vaultStore';
import { renderMarkdownMultiLine } from '../../engine/markdown';
import { Hash, Eye, Pencil } from 'lucide-react';
import { useI18n } from '../../i18n';

interface Props {
  block: CapsuleBlock & { type: 'heading' };
  onChange: (updatedData: HeadingBlockData) => void;
  onDelete: () => void;
  onOpenCapsule?: (messageId: string) => void;
}

export const HeadingBlock: React.FC<Props> = ({ block, onChange, onDelete, onOpenCapsule }) => {
  const [showPreview, setShowPreview] = useState(false);
  const { t } = useI18n();
  const { level, text } = block.data;

  const fontClasses = {
    1: 'text-2xl font-bold text-slate-100 tracking-tight',
    2: 'text-xl font-semibold text-slate-200 tracking-tight',
    3: 'text-lg font-medium text-slate-300',
  }[level || 1];

  return (
    <div className="group relative my-2 flex items-baseline gap-2">
      <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
        <button
          type="button"
          onClick={() => {
            const nextLevel = level === 3 ? 1 : ((level + 1) as 1 | 2 | 3);
            onChange({ ...block.data, level: nextLevel });
          }}
          className="p-1 text-xs font-mono text-slate-500 hover:text-cyan-400 bg-slate-800/80 rounded border border-slate-700 flex items-center gap-0.5"
          title="Toggle Heading Level (H1 -> H2 -> H3)"
        >
          <Hash className="w-3 h-3" />
          <span>{level}</span>
        </button>
        <button
          type="button"
          onClick={() => setShowPreview((v) => !v)}
          className="p-1 text-xs text-slate-500 hover:text-cyan-300 bg-slate-800/80 rounded border border-slate-700"
          title={showPreview ? t('block.edit') : t('block.preview')}
        >
          {showPreview ? <Pencil className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
        </button>
      </div>

      {showPreview ? (
        <div
          className={`w-full ${fontClasses} cursor-text`}
          onDoubleClick={() => setShowPreview(false)}
          title={t('block.doubleClickEdit')}
        >
          {renderMarkdownMultiLine(
            text,
            (q) => vaultStore.resolveBacklink(q),
            onOpenCapsule
          )}
        </div>
      ) : (
        <input
          type="text"
          value={text}
          onChange={(e) => onChange({ ...block.data, text: e.target.value })}
          placeholder={`Heading ${level}...`}
          className={`w-full bg-transparent border-none outline-none focus:ring-0 ${fontClasses} placeholder:text-slate-600`}
        />
      )}
    </div>
  );
};
