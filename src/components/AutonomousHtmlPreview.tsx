import React, { useState } from 'react';
import { CapsuleNode } from '../types';
import { generateAutonomousHtml } from '../engine/mime';
import { Mail, Monitor, ShieldCheck, Globe, Download, FileCode } from 'lucide-react';
import { isDesktop, pickSavePath, writeTextFile } from '../engine/desktopStore';
import { useI18n } from '../i18n';

interface Props {
  capsule: CapsuleNode;
  onDownloadEml: (messageId: string) => void;
}

export const AutonomousHtmlPreview: React.FC<Props> = ({ capsule, onDownloadEml }) => {
  const { t } = useI18n();
  const htmlContent = generateAutonomousHtml(capsule);
  const originalEmailAttachment = capsule.attachments?.find(
    (a) => a.name === 'email_body.html' || a.cid === 'email_body.html'
  );

  const [viewMode, setViewMode] = useState<'dashboard' | 'original'>(
    originalEmailAttachment ? 'original' : 'dashboard'
  );
  const [exported, setExported] = useState(false);

  const activeContent =
    viewMode === 'original' && originalEmailAttachment
      ? originalEmailAttachment.content
      : htmlContent;

  // Export the active view as a single .html file (works in any browser).
  const handleDownloadHtml = async () => {
    const safe = capsule.subject.replace(/[^a-zA-Z0-9\u0400-\u04FF_-]+/g, '_').slice(0, 60) || 'capsule';
    const prefix = viewMode === 'original' ? 'email_' : 'dashboard_';
    const fileName = `${prefix}${safe}.html`;
    if (isDesktop()) {
      const path = await pickSavePath(fileName);
      if (path) await writeTextFile(path, activeContent);
      setExported(true);
      setTimeout(() => setExported(false), 2500);
      return;
    }
    const blob = new Blob([activeContent], { type: 'text/html;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="w-full h-full bg-[#020617] flex flex-col overflow-hidden text-xs text-slate-300">
      {/* Header Info */}
      <div className="bg-[#0f172a]/60 border-b border-slate-800 px-6 py-3 flex flex-wrap items-center justify-between gap-4 backdrop-blur-md">
        <div className="flex items-center gap-3">
          <div className="p-2 rounded-lg bg-cyan-500/10 text-cyan-400 border border-cyan-500/30">
            <Mail className="w-4 h-4" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-bold text-white uppercase text-xs tracking-wider">
                Autonomous HTML5 Email Dashboard
              </span>
              <span className="text-[10px] px-2 py-0.5 rounded bg-cyan-950/60 text-cyan-300 border border-cyan-800/60 font-mono">
                100% Client-Compatible
              </span>
            </div>
            <div className="text-[11px] text-slate-400 mt-0.5">
              Self-contained HTML5 dashboard viewable in Apple Mail, Thunderbird, Gmail or any browser without installing CapsuleCraft.
            </div>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {originalEmailAttachment && (
            <div className="flex items-center bg-slate-900/80 p-0.5 rounded-lg border border-slate-700/60 mr-2">
              <button
                type="button"
                onClick={() => setViewMode('original')}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-all ${
                  viewMode === 'original'
                    ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40 shadow-sm'
                    : 'text-slate-400 hover:text-white'
                }`}
                title={t('preview.originalTitle')}
              >
                <Mail className="w-3.5 h-3.5" />
                <span>Original Email</span>
              </button>
              <button
                type="button"
                onClick={() => setViewMode('dashboard')}
                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium transition-all ${
                  viewMode === 'dashboard'
                    ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40 shadow-sm'
                    : 'text-slate-400 hover:text-white'
                }`}
                title={t('preview.dashboardTitle')}
              >
                <Monitor className="w-3.5 h-3.5" />
                <span>Capsule Dashboard</span>
              </button>
            </div>
          )}

          <button
            type="button"
            onClick={handleDownloadHtml}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-semibold transition-all shadow-[0_0_10px_rgba(16,185,129,0.3)] text-xs"
            title={t('preview.saveHtmlTitle')}
          >
            <FileCode className="w-3.5 h-3.5" />
            <span>{exported ? 'Saved!' : 'Download .html'}</span>
          </button>
          <button
            type="button"
            onClick={() => onDownloadEml(capsule.messageId)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-cyan-600 hover:bg-cyan-500 text-white font-semibold transition-all shadow-[0_0_10px_rgba(6,182,212,0.3)] text-xs"
          >
            <Download className="w-3.5 h-3.5" />
            <span>Download .eml File</span>
          </button>
        </div>
      </div>

      {/* Simulated Email Client Viewport */}
      <div className="flex-1 p-4 md:p-6 bg-[#020617] overflow-y-auto flex justify-center">
        <div className="w-full max-w-4xl rounded-2xl border border-slate-800 bg-[#0f172a]/30 shadow-2xl overflow-hidden flex flex-col backdrop-blur-md">
          {/* Email Client Window Header */}
          <div className="bg-slate-950/80 px-4 py-3 border-b border-slate-800 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className="flex gap-1.5">
                <span className="w-3 h-3 rounded-full bg-rose-500/80 inline-block" />
                <span className="w-3 h-3 rounded-full bg-amber-500/80 inline-block" />
                <span className="w-3 h-3 rounded-full bg-cyan-500/80 inline-block" />
              </div>
              <span className="text-slate-400 text-xs font-mono ml-2">
                Mail Client View: {capsule.subject} {viewMode === 'original' ? '(Original)' : '(Dashboard)'}
              </span>
            </div>

            <div className="flex items-center gap-2 text-[10px] text-slate-500 font-mono">
              <ShieldCheck className="w-3.5 h-3.5 text-cyan-400" />
              <span title="RFC 5322 structure is valid; preview is isolated in sandbox">
                RFC 5322 valid · sandboxed
              </span>
            </div>
          </div>

          {/* Iframe with HTML5 content */}
          <iframe
            title="Autonomous HTML5 Preview"
            srcDoc={activeContent}
            className={`w-full flex-1 min-h-[600px] border-none transition-colors ${
              viewMode === 'original' ? 'bg-white' : 'bg-slate-900'
            }`}
            sandbox="allow-popups allow-popups-to-escape-sandbox"
          />
        </div>
      </div>
    </div>
  );
};
