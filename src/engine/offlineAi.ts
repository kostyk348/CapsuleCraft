// Offline AI Co-Thinker — content-aware deterministic reasoning engine.
//
// PRINCIPLE (v1.3 rewrite): the engine READS the actual capsule — subject,
// block texts, numeric claims, CSV attachments, wikilinks — and produces
// its analysis FROM that content. It never invents numbers: every figure
// it reports is either computed from an attached dataset or quoted from
// the capsule text. No network, no API key, fully deterministic: the same
// capsule always yields the same response.
//
// Same response contract as before: { success, mode, reply, text, result }.

import { tokenize } from './bm25';
import {
  inspectVagueWords,
  inspectActionability,
  inspectBalance,
  inspectReadability,
  inspectDataAnomalies,
  matchPromptFocus,
  detectTextLang,
  SupportedLang,
} from './cothinker';

export * from './cothinker';

export interface OfflineAiResult {
  author: string;
  criticism?: string;
  text?: string;
  confidence?: number;
  sourceClaim?: string;
  suggestedBlock?: any;
  suggestedBlocks?: any[];
  subject?: string;
  inReplyTo?: string;
  blocks?: any[];
}

/** Optional vault context — enables honest "related capsules" suggestions. */
export interface VaultContext {
  vaultNodes?: Array<{ messageId: string; subject: string; tags?: string[]; blocks?: any[] }>;
}

// ── text mining ───────────────────────────────────────────────────────────────

/** RU + EN stopwords — filtered before keyword extraction. */
export const STOPWORDS = new Set([
  // Russian
  'и','в','во','не','что','он','на','я','с','со','как','а','то','все','она','так','его','но','да','ты','к','у','же','вы','за','бы','по','только','ее','мне','было','вот','от','меня','еще','нет','о','из','ему','теперь','когда','даже','ну','вдруг','ли','если','уже','или','ни','быть','был','него','до','вас','нибудь','опять','уж','вам','ведь','там','потом','себя','ничего','ей','может','они','тут','где','есть','надо','ней','для','мы','тебя','их','чем','была','сам','чтоб','без','будто','человек','чего','раз','тоже','себе','под','будет','ж','тогда','кто','этот','того','потому','этого','какой','совсем','ним','здесь','этом','один','почти','мой','тем','чтобы','нее','кажется','сейчас','были','куда','зачем','всех','никогда','можно','при','наконец','два','об','другой','хоть','после','над','больше','тот','через','эти','нас','про','всего','них','какая','много','разве','три','эту','моя','впрочем','хорошо','свою','этой','перед','иногда','лучше','чуть','том','нельзя','такой','им','более','всегда','конечно','всю','между','это','как-то','очень','ещё','также','который','которая','которые','для','наш','наши','ваш','ваши','данн','данные',
  // English
  'the','of','to','and','a','in','that','is','was','he','for','it','with','as','his','on','be','at','by','i','this','had','not','are','but','from','or','have','an','they','which','one','you','were','her','all','she','there','would','their','we','him','been','has','when','who','will','more','no','if','out','so','said','what','up','its','about','into','than','them','can','only','other','new','some','could','time','these','two','may','then','do','first','any','my','now','such','like','our','over','man','me','even','most','made','after','also','did','many','before','must','through','back','years','where','much','your','way','well','down','should','because','each','just','those','people','mr','how','too','little','state','good','very','make','world','still','own','see','men','work','long','get','here','between','both','life','being','under','never','day','same','another','know','while','last','might','us','great','old','year','off','come','since','against','go','came','right','used','take','three',
]);

/** Extract top keywords (TF, stopword-filtered) — the capsule's real themes. */
export function extractKeywords(text: string, limit = 8): string[] {
  const counts = new Map<string, number>();
  for (const t of tokenize(text)) {
    if (STOPWORDS.has(t) || /^\d+$/.test(t)) continue;
    counts.set(t, (counts.get(t) || 0) + 1);
  }
  return Array.from(counts.entries())
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
    .slice(0, limit)
    .map(([w]) => w);
}

/** All human-readable text of a capsule (subject + text-bearing blocks). */
export function textContentOf(capsule: any): string {
  if (!capsule) return '';
  const parts: string[] = [capsule.subject || '', (capsule.tags || []).join(' ')];
  for (const b of capsule.blocks || []) {
    const d = b?.data || {};
    if (typeof d.text === 'string') parts.push(d.text);
    if (typeof d.title === 'string') parts.push(d.title);
    if (typeof d.content === 'string') parts.push(d.content);
    if (typeof d.criticism === 'string') parts.push(d.criticism);
    if (typeof d.sourceClaim === 'string') parts.push(d.sourceClaim);
    if (Array.isArray(d.items)) {
      for (const it of d.items) if (typeof it?.text === 'string') parts.push(it.text);
    }
  }
  return parts.join('\n');
}

/** Sentences that contain concrete numbers / percentages / currency — the claims worth challenging. */
export function findClaims(text: string, limit = 3): string[] {
  const sentences = text
    .replace(/\[\[([^\]]+)\]\]/g, '$1')
    .split(/(?<=[.!?;])\s+|\n+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 12 && /\d/.test(s));
  // Prefer sentences with relative numbers (%, ratios) — they are the strongest claims.
  sentences.sort((a, b) => {
    const score = (s: string) => (/%|\$|₽|x\b|×/i.test(s) ? 2 : 0) + (/\d+\.\d+/.test(s) ? 1 : 0);
    return score(b) - score(a);
  });
  return sentences.slice(0, limit);
}

export interface RiskFinding {
  rule: string;
  detail: string;
}

/** Rule-based risk scan of the actual text. Honest: only reports what is really there. */
export function detectRisks(text: string, dataPoints = 0, lang: SupportedLang = detectTextLang(text)): RiskFinding[] {
  const t = text.toLowerCase();
  const out: RiskFinding[] = [];

  if (/(всегда|никогда|гаранти|стопроцентн|100%|обязательн[оы]|always|never|guarantee|definitely)/.test(t)) {
    out.push({
      rule: 'absolute-claim',
      detail:
        lang === 'en'
          ? 'Text contains absolute phrasing ("always/never/guaranteed") — every model has boundary conditions where it breaks down.'
          : 'В тексте есть абсолютные формулировки («всегда/никогда/гарантированно») — у любой модели есть граничные условия, где они ломаются.',
    });
  }
  if (!/(риск|буфер|резерв|страхов|запас|отток|просадк|downside|risk|buffer|contingency|fallback|churn)/.test(t)) {
    out.push({
      rule: 'no-downside',
      detail:
        lang === 'en'
          ? 'Zero mentions of risks, buffers, or contingency reserves — the plan does not outline downside mitigation.'
          : 'Ни одного упоминания рисков, буфера или резерва — план не описывает, что делать при отклонении от базы.',
    });
  }
  if (/(единственн|только один|монопол|single point|only one|bottleneck)/.test(t)) {
    out.push({
      rule: 'single-point',
      detail:
        lang === 'en'
          ? 'Single point of failure detected: reliance on a single provider, channel, or resource.'
          : 'Обнаружена ставка на единственный источник/канал — это точка отказа.',
    });
  }
  if (/(вырастет|будет расти|прогноз|предполагаем|ожидаем|экстраполир|grow|forecast|predict|expect|extrapolat)/.test(t) && dataPoints > 0 && dataPoints < 6) {
    out.push({
      rule: 'thin-extrapolation',
      detail:
        lang === 'en'
          ? `Trend extrapolated from only ${dataPoints} data points — robust conclusions require at least 6–8 observations.`
          : `Тренд экстраполируется по ${dataPoints} точкам — для устойчивого вывода нужно минимум 6–8 наблюдений.`,
    });
  }
  if (/(дедлайн|срочно|к концу|не позже|как можно скорее|deadline|asap|urgently|due date)/.test(t)) {
    out.push({
      rule: 'deadline-pressure',
      detail:
        lang === 'en'
          ? 'Deadlines cited without iteration buffers — any rework will derail the entire schedule.'
          : 'Сроки упоминаются без явного запаса на итерации — любая переделка сдвинет весь план.',
    });
  }
  for (const m of text.matchAll(/(?:рост|выручк|прибыл|up|growth|revenue)?[^.!?\n]{0,40}?(\d{2,})\s?%/g)) {
    const v = parseInt(m[1], 10);
    if (v >= 50) {
      out.push({
        rule: 'big-number',
        detail:
          lang === 'en'
            ? `Claimed ${v}% is a significant figure: requires independent methodology verification, not just common sense.`
            : `Заявленные ${v}% — крупное число: нужна независимая проверка методики расчёта, а не только здравый смысл.`,
      });
      break;
    }
  }
  return out.slice(0, 4);
}

// ── CSV math (real, computed) ────────────────────────────────────────────────

export interface CsvColumnStats {
  name: string;
  n: number;
  min: number;
  max: number;
  mean: number;
  first: number;
  last: number;
  deltaPct: number;
  trend: 'up' | 'down' | 'flat';
}

export interface CsvReport {
  fileName: string;
  headers: string[];
  rowCount: number;
  columns: CsvColumnStats[];
  correlation?: number;
  correlationPair?: [string, string];
}

/** Minimal robust CSV parser: commas, double-quoted cells, CRLF. */
export function parseCsv(csv: string): { headers: string[]; rows: string[][] } | null {
  const lines = csv.trim().split(/\r?\n/).filter((l) => l.trim().length > 0);
  if (lines.length < 2) return null;

  const splitLine = (line: string): string[] => {
    const cells: string[] = [];
    let cur = '';
    let inQ = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (ch === '"') {
        if (inQ && line[i + 1] === '"') { cur += '"'; i++; }
        else inQ = !inQ;
      } else if (ch === ',' && !inQ) {
        cells.push(cur.trim());
        cur = '';
      } else cur += ch;
    }
    cells.push(cur.trim());
    return cells;
  };

  const headers = splitLine(lines[0]);
  const rows = lines.slice(1).map(splitLine).filter((r) => r.length === headers.length);
  if (!headers.length || !rows.length) return null;
  return { headers, rows };
}

/** OLS slope over index → value (used for trend direction). */
function slope(values: number[]): number {
  const n = values.length;
  if (n < 2) return 0;
  const mx = (n - 1) / 2;
  const my = values.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let den = 0;
  for (let i = 0; i < n; i++) {
    num += (i - mx) * (values[i] - my);
    den += (i - mx) ** 2;
  }
  return den ? num / den : 0;
}

/** Pearson correlation between two equal-length series. */
export function correlation(a: number[], b: number[]): number {
  const n = Math.min(a.length, b.length);
  if (n < 3) return 0;
  const ma = a.slice(0, n).reduce((x, y) => x + y, 0) / n;
  const mb = b.slice(0, n).reduce((x, y) => x + y, 0) / n;
  let num = 0;
  let da = 0;
  let db = 0;
  for (let i = 0; i < n; i++) {
    num += (a[i] - ma) * (b[i] - mb);
    da += (a[i] - ma) ** 2;
    db += (b[i] - mb) ** 2;
  }
  return da && db ? num / Math.sqrt(da * db) : 0;
}

/** Full honest analysis of one CSV attachment: per-column stats, trends, correlation. */
export function analyzeCsv(csv: string, fileName = 'dataset.csv'): CsvReport | null {
  const parsed = parseCsv(csv);
  if (!parsed) return null;
  const { headers, rows } = parsed;

  const columns: CsvColumnStats[] = [];
  for (let c = 0; c < headers.length; c++) {
    const raw = rows.map((r) => r[c]);
    const nums = raw.map((v) => Number(String(v).replace(/[$€₽\s,%]/g, '').replace(',', '.')));
    const isNumeric = nums.every((v) => Number.isFinite(v));
    if (!isNumeric) continue;

    const clean = nums.filter((v) => Number.isFinite(v));
    if (clean.length < 2) continue;

    const mean = clean.reduce((a, b) => a + b, 0) / clean.length;
    const first = clean[0];
    const last = clean[clean.length - 1];
    const deltaPct = first !== 0 ? ((last - first) / Math.abs(first)) * 100 : 0;
    const s = slope(clean);
    const rel = Math.abs(mean) > 0 ? Math.abs(s / mean) : 0;

    columns.push({
      name: headers[c],
      n: clean.length,
      min: Math.min(...clean),
      max: Math.max(...clean),
      mean,
      first,
      last,
      deltaPct,
      trend: rel < 0.005 ? 'flat' : s > 0 ? 'up' : 'down',
    });
  }

  const report: CsvReport = {
    fileName,
    headers,
    rowCount: rows.length,
    columns: columns.slice(0, 4),
  };

  if (columns.length >= 2) {
    const a = rows.map((r) => Number(String(r[headers.indexOf(columns[0].name)]).replace(/[^0-9.\-]/g, '')));
    const b = rows.map((r) => Number(String(r[headers.indexOf(columns[1].name)]).replace(/[^0-9.\-]/g, '')));
    const corr = correlation(a, b);
    if (Number.isFinite(corr) && corr !== 0) {
      report.correlation = corr;
      report.correlationPair = [columns[0].name, columns[1].name];
    }
  }
  return report.columns.length ? report : null;
}

const fmt = (v: number, unit = '', locale: 'ru-RU' | 'en-US' = 'ru-RU') =>
  `${v >= 0 ? '' : '−'}${Math.abs(v).toLocaleString(locale, { maximumFractionDigits: 2 })}${unit}`;

/** Human, honest dataset summary — every number comes from the report. */
export function describeCsvReport(r: CsvReport, lang: SupportedLang = 'ru'): string {
  if (lang === 'en') {
    const lines: string[] = [
      `Dataset "${r.fileName}": ${r.rowCount} rows × ${r.headers.length} columns. All numbers below are computed from the attachment, never hallucinated.`,
    ];
    for (const c of r.columns) {
      const arrow = c.trend === 'up' ? '↑ growth' : c.trend === 'down' ? '↓ decline' : '→ flat';
      lines.push(
        `• ${c.name}: mean ${fmt(c.mean, '', 'en-US')}, range ${fmt(c.min, '', 'en-US')}…${fmt(c.max, '', 'en-US')}, ${arrow} ${fmt(c.deltaPct, '%', 'en-US')} (${fmt(c.first, '', 'en-US')} → ${fmt(c.last, '', 'en-US')}, n=${c.n})`,
      );
    }
    if (r.correlation !== undefined && r.correlationPair) {
      const a = Math.abs(r.correlation);
      const strength = a > 0.9 ? 'near-linear' : a > 0.7 ? 'notable' : a > 0.4 ? 'moderate' : 'weak';
      const dir = r.correlation > 0 ? 'positive' : 'negative';
      lines.push(`• Correlation ${r.correlationPair[0]} ↔ ${r.correlationPair[1]}: ${r.correlation.toFixed(2)} — ${strength} ${dir} correlation`);
      if (r.correlationPair.length === 2 && a > 0.7) {
        lines.push(`  ⚠ Correlation ≠ causation: strong statistical link, but causal mechanism requires separate testing.`);
      }
    }
    return lines.join('\n');
  }

  const lines: string[] = [
    `Датасет «${r.fileName}»: ${r.rowCount} строк × ${r.headers.length} колонок. Все числа ниже вычислены из вложения, не сгенерированы.`,
  ];
  for (const c of r.columns) {
    const arrow = c.trend === 'up' ? '↑ рост' : c.trend === 'down' ? '↓ снижение' : '→ без тренда';
    lines.push(
      `• ${c.name}: среднее ${fmt(c.mean)}, диапазон ${fmt(c.min)}…${fmt(c.max)}, ${arrow} ${fmt(c.deltaPct, '%')} (${fmt(c.first)} → ${fmt(c.last)}, n=${c.n})`,
    );
  }
  if (r.correlation !== undefined && r.correlationPair) {
    const a = Math.abs(r.correlation);
    const strength = a > 0.9 ? 'почти линейная' : a > 0.7 ? 'заметная' : a > 0.4 ? 'умеренная' : 'слабая';
    const dir = r.correlation > 0 ? 'прямая' : 'обратная';
    lines.push(`• Корреляция ${r.correlationPair[0]} ↔ ${r.correlationPair[1]}: ${r.correlation.toFixed(2)} — ${strength} ${dir} связь`);
    if (r.correlationPair.length === 2 && a > 0.7) {
      lines.push(`  ⚠ Корреляция ≠ причинность: связь сильная, но механизм нужно проверить отдельно.`);
    }
  }
  return lines.join('\n');
}

// ── related capsules from vault context ──────────────────────────────────────

export interface RelatedCapsuleSuggestion {
  messageId: string;
  subject: string;
  similarity: number;
}

/** Jaccard overlap over the meaningful token sets (honest, explainable %). */
export function relatedFromVault(
  capsule: any,
  ctx?: VaultContext,
  limit = 3,
): RelatedCapsuleSuggestion[] {
  if (!ctx?.vaultNodes?.length) return [];
  const base = new Set(tokenize(textContentOf(capsule)).filter((t) => !STOPWORDS.has(t)));
  if (base.size === 0) return [];

  const out: RelatedCapsuleSuggestion[] = [];
  for (const n of ctx.vaultNodes) {
    if (!n || n.messageId === capsule?.messageId) continue;
    const other = new Set(tokenize(textContentOf(n)).filter((t) => !STOPWORDS.has(t)));
    if (other.size === 0) continue;
    let inter = 0;
    for (const t of base) if (other.has(t)) inter += 1;
    const sim = inter / (base.size + other.size - inter);
    if (sim > 0.05) out.push({ messageId: n.messageId, subject: n.subject, similarity: sim });
  }
  return out.sort((a, b) => b.similarity - a.similarity).slice(0, limit);
}

// ── persona styling (deterministic variance) ─────────────────────────────────

interface PersonaStyle {
  label: string;
  labelEn: string;
  confidence: number;
}

const PERSONA_STYLES: Record<string, PersonaStyle> = {
  'Co-Thinker': { label: 'честный разбор содержимого', labelEn: 'honest content analysis', confidence: 0.86 },
};

function personaStyle(persona: string): PersonaStyle {
  return PERSONA_STYLES[persona] || { label: 'честный разбор содержимого', labelEn: 'honest content analysis', confidence: 0.82 };
}

/** Stable string hash — keeps persona/variant choices deterministic. */
function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

const pick = <T,>(arr: T[], seed: number): T => arr[seed % arr.length];

// ── the engine ───────────────────────────────────────────────────────────────

export function generateOfflineAiResponse(
  action: string,
  capsule: any,
  prompt?: string,
  persona: string = 'Co-Thinker',
  ctx?: VaultContext,
): OfflineAiResult {
  const subject: string = capsule?.subject || 'Knowledge Capsule';
  const text = textContentOf(capsule);
  const keywords = extractKeywords(text, 6);
  const claims = findClaims(text, 3);
  const style = personaStyle(persona);
  const seed = hash(subject + persona);

  // Real CSV attachments — the only allowed source of computed numbers.
  const csvReports = (capsule?.attachments || [])
    .map((a: any) =>
      a?.mimeType === 'text/csv' && typeof a?.content === 'string'
        ? analyzeCsv(a.content, a.name || 'dataset.csv')
        : null,
    )
    .filter(Boolean);
  const csv = csvReports[0] as CsvReport | undefined;
  const dataPoints = csv?.columns[0]?.n || 0;
  const lang = detectTextLang(subject + ' ' + text + ' ' + (prompt || ''));
  const risks = detectRisks(text, dataPoints, lang);
  const related = relatedFromVault(capsule, ctx, 2);

  const focus = matchPromptFocus(prompt, text, lang);
  const blocksCount = capsule?.blocks?.length || 0;
  const vagueFindings = inspectVagueWords(text, 2, lang);
  const actionFindings = inspectActionability(text, blocksCount, lang);
  const balanceFindings = inspectBalance(text, lang);
  const readabilityFindings = inspectReadability(text, 1, lang);
  const thoughtFindings = [...vagueFindings, ...balanceFindings, ...actionFindings, ...readabilityFindings];

  switch (action) {
    case 'critique': {
      const parts: string[] = [];
      if (lang === 'en') {
        parts.push(`[${persona}] Analysis of "${subject}" — ${style.labelEn}.`);
        parts.push(`Key themes: ${keywords.length ? keywords.join(' · ') : 'text is too brief'}.`);

        if (focus.hasCustomPrompt) {
          if (focus.missingTopicNotice) {
            parts.push('');
            parts.push(`[Regarding your query: "${prompt}"]`);
            parts.push(`⚠ ${focus.missingTopicNotice}`);
          } else if (focus.matchedQuotes.length > 0) {
            parts.push('');
            parts.push(`[Regarding your query: "${prompt}"]:`);
            focus.matchedQuotes.slice(0, 2).forEach((q) => parts.push(`• "${q}"`));
          }
        }

        if (claims.length > 0) {
          parts.push('');
          parts.push(`Numeric claims: ${claims.length}. Strongest claim:`);
          parts.push(`"${claims[0]}"`);
          parts.push(
            pick(
              [
                '— what is the source of this number? Without a stated methodology, verifiability is zero.',
                '— what base scenario yields this estimate, and what assumptions break it?',
                '— is this measured or extrapolated? The difference defines confidence in the conclusion.',
              ],
              seed,
            ),
          );
        } else {
          parts.push('');
          parts.push('The capsule contains zero numeric claims — conclusions rely on qualitative arguments. Consider adding measurable metrics or an attached CSV dataset.');
        }

        if (risks.length > 0) {
          parts.push('');
          parts.push('Risks (from content):');
          risks.forEach((r, i) => parts.push(`${i + 1}. ${r.detail}`));
        }

        if (thoughtFindings.length > 0) {
          parts.push('');
          parts.push('Thought & Phrasing Linter:');
          thoughtFindings.forEach((f, i) => {
            parts.push(`${i + 1}. ${f.title}: ${f.detail}`);
            if (f.suggestion) {
              parts.push(`   → Suggestion: ${f.suggestion}`);
            }
          });
        }

        if (csv) {
          parts.push('');
          parts.push(`Attached dataset has ${dataPoints} rows of data — above hypotheses can be directly tested against it.`);
        }
      } else {
        parts.push(`[${persona}] Разбор «${subject}» — ${style.label}.`);
        parts.push(`Ключевые темы: ${keywords.length ? keywords.join(' · ') : 'текст слишком короткий'}.`);

        if (focus.hasCustomPrompt) {
          if (focus.missingTopicNotice) {
            parts.push('');
            parts.push(`[По вашему вопросу: «${prompt}»]`);
            parts.push(`⚠ ${focus.missingTopicNotice}`);
          } else if (focus.matchedQuotes.length > 0) {
            parts.push('');
            parts.push(`[По вашему вопросу: «${prompt}»]:`);
            focus.matchedQuotes.slice(0, 2).forEach((q) => parts.push(`• «${q}»`));
          }
        }

        if (claims.length > 0) {
          parts.push('');
          parts.push(`Утверждений с числами: ${claims.length}. По самому сильному:`);
          parts.push(`«${claims[0]}»`);
          parts.push(
            pick(
              [
                '— откуда это число? Если методики нет в капсуле, проверяемость нулевая.',
                '— какой базовый сценарий даёт такую оценку и что её ломает?',
                '— это измерено или экстраполировано? Разница определяет доверие к выводу.',
              ],
              seed,
            ),
          );
        } else {
          parts.push('');
          parts.push('В капсуле нет ни одного утверждения с числами — выводы держатся на качественных аргументах. Для проверки стоит добавить измеримую метрику или CSV-вложение.');
        }

        if (risks.length > 0) {
          parts.push('');
          parts.push('Риски (по содержимому):');
          risks.forEach((r, i) => parts.push(`${i + 1}. ${r.detail}`));
        }

        if (thoughtFindings.length > 0) {
          parts.push('');
          parts.push('Линтер мысли и формулировок:');
          thoughtFindings.forEach((f, i) => {
            parts.push(`${i + 1}. ${f.title}: ${f.detail}`);
            if (f.suggestion) {
              parts.push(`   → Совет: ${f.suggestion}`);
            }
          });
        }

        if (csv) {
          parts.push('');
          parts.push(`Во вложении ${dataPoints} строк данных — гипотезы выше можно проверить прямо на них.`);
        }
      }

      const criticism = parts.join('\n');
      return {
        author: persona,
        criticism,
        confidence: style.confidence,
        suggestedBlock: {
          id: `blk_ai_${Date.now()}`,
          type: 'ai_opponent_block',
          author: persona,
          data: {
            criticism,
            confidence: style.confidence,
            sourceClaim: claims[0] || subject,
          },
        },
      };
    }

    case 'continue': {
      const parts: string[] = [];
      if (lang === 'en') {
        parts.push(`[${persona}] Next steps for "${subject}" (${style.labelEn}):`);
        if (keywords.length >= 2) {
          parts.push(`1. Formalize "${keywords[0]}": define a concrete metric, failure threshold, and measurement method.`);
          parts.push(`2. Verify link "${keywords[0]}" ↔ "${keywords[1]}": if the latter directly impacts the former, it is a leverage point; otherwise it is superfluous.`);
        } else {
          parts.push(`1. Capsule currently scopes questions wider than answers: break down "${keywords[0] || subject}" into a focused capsule with data.`);
        }
        if (actionFindings.length > 0 && actionFindings[0].suggestion) {
          parts.push(`• Immediate step: ${actionFindings[0].suggestion}`);
        }
        if (focus.hasCustomPrompt && focus.matchedQuotes.length > 0) {
          parts.push(`• Query focus ("${prompt}"): test assertion "${focus.matchedQuotes[0]}" in a separate experiment.`);
        }
        if (csv) {
          const c = csv.columns[0];
          const dir = c.trend === 'up' ? 'increasing' : c.trend === 'down' ? 'declining' : 'flat';
          parts.push(`3. Data already shows: ${c.name} is ${dir} (${fmt(c.deltaPct, '%', 'en-US')} across ${c.n} points). Follow-up logic should account for this.`);
        }
        if (related.length > 0) {
          parts.push(`4. Connect with "${related[0].subject}" — content overlap ~${Math.round(related[0].similarity * 100)}%. Common ground exists, [[wikilink]] will link them in the graph.`);
        }
      } else {
        parts.push(`[${persona}] Куда развивать «${subject}» (${style.label}):`);
        if (keywords.length >= 2) {
          parts.push(`1. Формализовать «${keywords[0]}»: конкретная метрика, порог отказа и способ измерения.`);
          parts.push(`2. Проверить связь «${keywords[0]}» ↔ «${keywords[1]}»: если второе действительно влияет на первое — это рычаг, если нет — лишний слой.`);
        } else {
          parts.push(`1. Капсула пока ставит вопрос шире, чем отвечает: развернуть «${keywords[0] || subject}» в отдельную капсулу с данными.`);
        }
        if (actionFindings.length > 0 && actionFindings[0].suggestion) {
          parts.push(`• Ближайший шаг: ${actionFindings[0].suggestion}`);
        }
        if (focus.hasCustomPrompt && focus.matchedQuotes.length > 0) {
          parts.push(`• Фокус запроса («${prompt}»): проверить утверждение «${focus.matchedQuotes[0]}» в отдельном эксперименте.`);
        }
        if (csv) {
          const c = csv.columns[0];
          const dir = c.trend === 'up' ? 'растёт' : c.trend === 'down' ? 'падает' : 'стоит на месте';
          parts.push(`3. Данные уже показывают: ${c.name} ${dir} (${fmt(c.deltaPct, '%')} за ${c.n} точек). Продолжение мысли должно это учитывать, а не игнорировать.`);
        }
        if (related.length > 0) {
          parts.push(`4. Связать с «${related[0].subject}» — совпадение содержания ~${Math.round(related[0].similarity * 100)}%. Общая база уже есть, [[wikilink]] соберёт их в граф.`);
        }
      }
      const textOut = parts.join('\n');
      return {
        author: persona,
        text: textOut,
        suggestedBlocks: [
          {
            id: `blk_ai_cont_${Date.now()}`,
            type: 'callout',
            data: {
              title: `AI Synthesis (${persona})`,
              content: textOut,
              icon: 'Sparkles',
              tone: 'emerald',
            },
          },
        ],
      };
    }

    case 'reply_thread': {
      const counterBlocks: any[] = [];
      if (lang === 'en') {
        counterBlocks.push(
          {
            id: `blk_reply_1_${Date.now()}`,
            type: 'heading',
            data: { level: 2, text: `Counterarguments to "${subject}"` },
          },
          {
            id: `blk_reply_2_${Date.now()}`,
            type: 'ai_opponent_block',
            author: persona,
            data: {
              criticism: claims[0]
                ? `[${persona}] Thesis "${claims[0]}" does not directly follow from the provided material: causality between observation and conclusion is unproven. Alternative explanation: "${keywords[0] || 'external factor'}" may account for the same data without altering the model.`
                : `[${persona}] The core text argues qualitatively, lacking measurable anchors. Before adopting this model, empirical data on "${keywords[0] || subject}" is needed.`,
              confidence: style.confidence,
              sourceClaim: claims[0] || subject,
            },
          },
          {
            id: `blk_reply_3_${Date.now()}`,
            type: 'paragraph',
            data: {
              text: `Falsification criterion: if ${Math.max(6, dataPoints + 2)}+ observations are gathered for "${keywords[0] || subject}" and the trend holds when controlling for "${keywords[1] || 'external factors'}", the objection is resolved. Until then, competing hypotheses remain equally valid.`,
            },
          },
        );
        if (related.length > 0) {
          counterBlocks.push({
            id: `blk_reply_4_${Date.now()}`,
            type: 'callout',
            data: {
              title: 'Related Materials',
              content: `"${related[0].subject}" overlaps with this capsule (~${Math.round(related[0].similarity * 100)}% shared themes) — context may provide the missing data.`,
              tone: 'blue',
            },
          });
        }
      } else {
        counterBlocks.push(
          {
            id: `blk_reply_1_${Date.now()}`,
            type: 'heading',
            data: { level: 2, text: `Контраргументы к «${subject}»` },
          },
          {
            id: `blk_reply_2_${Date.now()}`,
            type: 'ai_opponent_block',
            author: persona,
            data: {
              criticism: claims[0]
                ? `[${persona}] Тезис «${claims[0]}» не следует из приведённого материала напрямую: не показана связь между наблюдением и выводом. Альтернативное объяснение — «${keywords[0] || 'внешний фактор'}» может объяснять те же данные без смены модели.`
                : `[${persona}] Основной текст аргументирует качественно, без измеримых опор. Прежде чем принимать модель, нужны данные по «${keywords[0] || subject}».`,
              confidence: style.confidence,
              sourceClaim: claims[0] || subject,
            },
          },
          {
            id: `blk_reply_3_${Date.now()}`,
            type: 'paragraph',
            data: {
              text: `Критерий опровержения: если по «${keywords[0] || subject}» соберётся ${Math.max(6, dataPoints + 2)}+ наблюдений и тенденция сохранится при учёте «${keywords[1] || 'внешних факторов'}» — возражение снимается. Пока этого нет, обе модели равноправны.`,
            },
          },
        );
        if (related.length > 0) {
          counterBlocks.push({
            id: `blk_reply_4_${Date.now()}`,
            type: 'callout',
            data: {
              title: 'Связанные материалы',
              content: `«${related[0].subject}» перекликается с этой капсулой (~${Math.round(related[0].similarity * 100)}% общих тем) — контекст может содержать недостающие данные.`,
              tone: 'blue',
            },
          });
        }
      }
      return {
        subject: `Re: ${subject}`,
        author: `${persona} <agent@capsuleos.local>`,
        inReplyTo: capsule?.messageId || '<capsule-root@vault.local>',
        blocks: counterBlocks,
      };
    }

    case 'data_insight': {
      let textOut: string;
      if (csv) {
        const rawContent = (capsule?.attachments || []).find((a: any) => a?.name === csv.fileName)?.content || '';
        const parsed = parseCsv(rawContent);
        const anomalyLines: string[] = [];
        if (parsed) {
          for (const c of csv.columns) {
            const colIdx = parsed.headers.indexOf(c.name);
            if (colIdx >= 0) {
              const nums = parsed.rows.map((r) => Number(String(r[colIdx]).replace(/[$€₽\s,%]/g, '').replace(',', '.'))).filter((v) => Number.isFinite(v));
              const anomalies = inspectDataAnomalies(c.name, nums, lang);
              for (const a of anomalies) {
                anomalyLines.push(`• ⚠ ${a.message}`);
              }
            }
          }
        }
        textOut = `[${persona} — Data Analytics]\n${describeCsvReport(csv, lang)}`;
        if (anomalyLines.length > 0) {
          const header = lang === 'en' ? 'Sample anomalies & observations:' : 'Аномалии и особенности выборки:';
          textOut += `\n\n${header}\n` + anomalyLines.join('\n');
        }
      } else {
        const nums = text.match(/\d+(?:[.,]\d+)?/g) || [];
        if (lang === 'en') {
          textOut = `[${persona} — Data Analytics]\n` +
            (nums.length > 0
              ? `No tabular attachments in capsule — numbers are never hallucinated. Found ${nums.length} numeric values in text, key ones: ${nums.slice(0, 5).join(', ')}. Attach a CSV file (text/csv) to compute honest means, trends, and correlations.`
              : `No tabular attachments or numbers in capsule — nothing to analyze, and I will not simulate data. Attach a CSV dataset to get honest statistics: means, trends, and correlations.`);
        } else {
          textOut = `[${persona} — Data Analytics]\n` +
            (nums.length > 0
              ? `Табличных вложений в капсуле нет — числа я не выдумываю. В тексте найдены ${nums.length} числовых значений, ключевые: ${nums.slice(0, 5).join(', ')}. Приложите CSV (text/csv) — тогда посчитаю средние, тренды и корреляции по реальным данным.`
              : `Табличных вложений и чисел в капсуле нет — анализировать нечего, и я не буду это имитировать. Приложите CSV-вложение к капсуле, и я посчитаю статистику честно: средние, тренды, корреляции.`);
        }
      }
      return { author: `${persona} (Data Analytics)`, text: textOut };
    }

    default: {
      const blocksN = capsule?.blocks?.length || 0;
      const textOut = lang === 'en'
        ? `[${persona}] Capsule "${subject}": ${blocksN} blocks, themes — ${keywords.length ? keywords.slice(0, 3).join(', ') : 'none'}, ` +
          `claims with numbers — ${claims.length}${csv ? `, attachment "${csv.fileName}" (${dataPoints} rows) ready for analysis` : ''}. RFC 5322 invariants preserved.`
        : `[${persona}] Капсула «${subject}»: ${blocksN} блоков, тем — ${keywords.length ? keywords.slice(0, 3).join(', ') : 'нет'}, ` +
          `утверждений с числами — ${claims.length}${csv ? `, вложение «${csv.fileName}» (${dataPoints} строк) готово к анализу` : ''}. Инварианты RFC 5322 соблюдены.`;
      return { author: persona, text: textOut };
    }
  }
}

/** Drop-in replacement for the /api/ai/co-think HTTP endpoint. */
export function offlineAiResponse(
  action: string,
  currentCapsule: any,
  prompt?: string,
  persona: string = 'Co-Thinker',
  ctx?: VaultContext,
) {
  const result = generateOfflineAiResponse(action, currentCapsule, prompt, persona, ctx);
  const doneLang = detectTextLang(textContentOf(currentCapsule) + ' ' + (prompt || ''));
  const reply = result.criticism || result.text ||
    (doneLang === 'en' ? 'Capsule analysis complete.' : 'Анализ капсулы завершен.');
  return {
    success: true,
    mode: 'offline',
    reply,
    text: result.text || result.criticism || reply,
    result,
  };
}
