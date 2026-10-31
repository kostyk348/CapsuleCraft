import React, { useRef, useEffect, useState } from 'react';
import { CapsuleBlock, ParagraphBlockData } from '../../types';
import { vaultStore } from '../../engine/vaultStore';
import { renderMarkdownMultiLine } from '../../engine/markdown';
import { Eye, Pencil, CornerDownLeft } from 'lucide-react';
import { useI18n } from '../../i18n';

interface Props {
  block: CapsuleBlock & { type: 'paragraph' };
  onChange: (updatedData: ParagraphBlockData) => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onDelete: () => void;
  onOpenCapsule?: (messageId: string) => void;
}

interface Suggestion {
  messageId: string;
  subject: string;
}

/** Extract the query of a trailing unfinished [[wikilink. */
function trailingBacklinkQuery(text: string): string | null {
  const m = text.match(/\[\[([^\]]*)$/);
  if (!m) return null;
  return m[1].trim();
}

export const RichTextBlock: React.FC<Props> = ({ block, onChange, onKeyDown, onOpenCapsule }) => {
  const { t } = useI18n();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [showPreview, setShowPreview] = useState(false);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [suggestOpen, setSuggestOpen] = useState(false);

  const adjustHeight = () => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = `${Math.max(28, textareaRef.current.scrollHeight)}px`;
    }
  };

  useEffect(() => {
    adjustHeight();
  }, [block.data.text]);

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const text = e.target.value;
    onChange({ text });
    adjustHeight();

    // Backlink autocomplete: typing [[… opens a suggestion list.
    const q = trailingBacklinkQuery(text);
    if (q !== null) {
      const results = vaultStore.search(q).slice(0, 6);
      setSuggestions(results.map((r) => ({ messageId: r.messageId, subject: r.subject })));
      setSuggestOpen(results.length > 0);
    } else {
      setSuggestOpen(false);
    }
  };

  const applySuggestion = (s: Suggestion) => {
    const text = block.data.text;
    const m = text.match(/\[\[([^\]]*)$/);
    if (m) {
      const newText = text.slice(0, m.index) + `[[${s.subject}]]`;
      onChange({ text: newText });
    }
    setSuggestOpen(false);
  };

  return (
    <div className="relative my-1.5 group">
      <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity absolute -top-1 right-0 z-10">
        <button
          type="button"
          onClick={() => setShowPreview((v) => !v)}
          className="p-1 rounded bg-slate-800/80 text-slate-400 hover:text-cyan-300 border border-slate-700/60 text-[10px] flex items-center gap-1"
          title={showPreview ? t('block.edit') : t('block.preview')}
        >
          {showPreview ? <Pencil className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
        </button>
      </div>

      {showPreview ? (
        <div
          className="w-full text-slate-200 text-[15px] leading-relaxed py-0.5 cursor-text"
          onDoubleClick={() => setShowPreview(false)}
          onClick={() => setShowPreview(false)}
          title={t('block.doubleClickEdit')}
        >
          {renderMarkdownMultiLine(
            block.data.text,
            (q) => vaultStore.resolveBacklink(q),
            onOpenCapsule
          )}
        </div>
      ) : (
        <textarea
          ref={textareaRef}
          rows={1}
          value={block.data.text}
          onChange={handleChange}
          onKeyDown={(e) => {
            if (suggestOpen && (e.key === 'Enter' || e.key === 'Tab') && suggestions.length > 0) {
              e.preventDefault();
              applySuggestion(suggestions[0]);
              return;
            }
            onKeyDown?.(e);
          }}
          placeholder={t('block.paragraphPlaceholder')}
          className="w-full resize-none overflow-hidden bg-transparent text-slate-300 text-[15px] leading-relaxed border-none outline-none focus:ring-0 placeholder:text-slate-600 focus:text-slate-100 selection:bg-cyan-500/30 font-normal"
        />
      )}

      {/* Backlink autocomplete dropdown */}
      {suggestOpen && !showPreview && suggestions.length > 0 && (
        <div className="absolute left-0 right-0 top-full mt-0.5 z-30 bg-slate-900/95 backdrop-blur-md border border-cyan-500/30 rounded-lg shadow-xl overflow-hidden">
          {suggestions.map((s, i) => (
            <button
              key={s.messageId}
              type="button"
              onClick={() => applySuggestion(s)}
              className={`w-full flex items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-cyan-500/10 transition ${
                i === 0 ? 'text-cyan-300' : 'text-slate-300'
              }`}
            >
              <CornerDownLeft className="w-3 h-3 text-cyan-500/60 shrink-0" />
              <span className="truncate">{s.subject}</span>
              <span className="ml-auto text-[10px] font-mono text-slate-500 shrink-0">
                {s.messageId.replace(/^<|>$/g, '').split('@')[0]}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
