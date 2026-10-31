//! Search: Okapi BM25 for capsule relevance + command-palette ranking.
//!
//! Both are ports of the web engines (`src/engine/bm25.ts`,
//! `src/engine/commandPalette.ts`) and stay deterministic, dependency-free and
//! testable without a UI.

use std::collections::{BTreeMap, HashMap};

// ── Tokenizer ────────────────────────────────────────────────────────────────

/// Latin + Cyrillic + digits, `-`/`_` kept inside tokens, ≥ 2 chars.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let keep = ch.is_alphanumeric() || ch == '-' || ch == '_';
        if keep {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            if current.chars().count() >= 2 {
                tokens.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.chars().count() >= 2 {
        tokens.push(current);
    }
    tokens
}

// ── BM25 ─────────────────────────────────────────────────────────────────────

pub const BM25_K1: f32 = 1.5;
pub const BM25_B: f32 = 0.75;

#[derive(Default)]
pub struct Bm25Index {
    /// term -> doc -> term frequency
    postings: HashMap<String, HashMap<String, f32>>,
    /// doc -> term -> term frequency
    doc_tf: HashMap<String, HashMap<String, f32>>,
    doc_lengths: HashMap<String, f32>,
    total_length: f32,
    /// Terms sorted once per reindex so prefix scans stay predictable.
    sorted_terms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: String,
    pub score: f32,
}

impl Bm25Index {
    pub fn doc_count(&self) -> usize {
        self.doc_lengths.len()
    }

    pub fn term_count(&self) -> usize {
        self.postings.len()
    }

    pub fn avg_doc_length(&self) -> f32 {
        if self.doc_lengths.is_empty() {
            0.0
        } else {
            self.total_length / self.doc_lengths.len() as f32
        }
    }

    pub fn df(&self, term: &str) -> usize {
        self.postings.get(term).map(HashMap::len).unwrap_or(0)
    }

    pub fn idf(&self, term: &str) -> f32 {
        let df = self.df(term) as f32;
        let n = self.doc_count() as f32;
        if df == 0.0 || n == 0.0 {
            return 0.0;
        }
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    pub fn add(&mut self, id: &str, tokens: Vec<String>) {
        self.remove(id);
        let mut counts: HashMap<String, f32> = HashMap::new();
        for token in &tokens {
            *counts.entry(token.clone()).or_insert(0.0) += 1.0;
        }
        if !counts.is_empty() {
            for term in counts.keys() {
                self.postings
                    .entry(term.clone())
                    .or_default()
                    .insert(id.to_string(), counts[term]);
            }
            self.sorted_terms = self.postings.keys().cloned().collect();
            self.sorted_terms.sort();
        }
        self.total_length += tokens.len() as f32;
        self.doc_lengths.insert(id.to_string(), tokens.len() as f32);
        self.doc_tf.insert(id.to_string(), counts);
    }

    pub fn remove(&mut self, id: &str) {
        let Some(counts) = self.doc_tf.remove(id) else {
            return;
        };
        self.total_length -= self.doc_lengths.remove(id).unwrap_or(0.0);
        for term in counts.keys() {
            if let Some(docs) = self.postings.get_mut(term) {
                docs.remove(id);
                if docs.is_empty() {
                    self.postings.remove(term);
                }
            }
        }
        self.sorted_terms = self.postings.keys().cloned().collect();
        self.sorted_terms.sort();
    }

    pub fn clear(&mut self) {
        self.postings.clear();
        self.doc_tf.clear();
        self.doc_lengths.clear();
        self.sorted_terms.clear();
        self.total_length = 0.0;
    }

    fn saturation(&self, term: &str, id: &str) -> f32 {
        let f = self
            .doc_tf
            .get(id)
            .and_then(|counts| counts.get(term))
            .copied()
            .unwrap_or(0.0);
        if f == 0.0 {
            return 0.0;
        }
        let dl = self.doc_lengths.get(id).copied().unwrap_or(0.0);
        let avg = self.avg_doc_length();
        let norm = 1.0 - BM25_B + BM25_B * if avg > 0.0 { dl / avg } else { 0.0 };
        (f * (BM25_K1 + 1.0)) / (f + BM25_K1 * norm)
    }

    fn term_hits(&self, token: &str, prefix_weight: f32, max_prefix_terms: usize) -> Vec<Hit> {
        let mut scores: HashMap<&str, f32> = HashMap::new();
        if let Some(docs) = self.postings.get(token) {
            let idf = self.idf(token);
            for id in docs.keys() {
                *scores.entry(id.as_str()).or_insert(0.0) += idf * self.saturation(token, id);
            }
        }
        if prefix_weight > 0.0 {
            let mut used = 0;
            // Binary search the sorted term list instead of scanning the map.
            let start = self.sorted_terms.partition_point(|t| t.as_str() < token);
            for term in &self.sorted_terms[start..] {
                if !term.starts_with(token) {
                    break;
                }
                if term == token {
                    continue;
                }
                if used >= max_prefix_terms {
                    break;
                }
                used += 1;
                let weight = self.idf(term) * prefix_weight;
                if let Some(docs) = self.postings.get(term) {
                    for id in docs.keys() {
                        *scores.entry(id.as_str()).or_insert(0.0) +=
                            weight * self.saturation(term, id);
                    }
                }
            }
        }
        let mut hits: Vec<Hit> = scores
            .into_iter()
            .map(|(id, score)| Hit {
                id: id.to_string(),
                score,
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits
    }

    pub fn rank(&self, query: &str) -> Vec<Hit> {
        let tokens = tokenize(query);
        if tokens.is_empty() || self.doc_count() == 0 {
            return Vec::new();
        }
        let mut scores: HashMap<String, f32> = HashMap::new();
        for token in tokens {
            for hit in self.term_hits(&token, 0.5, 64) {
                *scores.entry(hit.id).or_insert(0.0) += hit.score;
            }
        }
        let mut hits: Vec<Hit> = scores
            .into_iter()
            .map(|(id, score)| Hit { id, score })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits
    }

    /// Cosine similarity of tf-idf vectors (0..1) — "related pages" without a model.
    pub fn similarity(&self, a: &str, b: &str) -> f32 {
        let (Some(da), Some(db)) = (self.doc_tf.get(a), self.doc_tf.get(b)) else {
            return 0.0;
        };
        let mut dot = 0.0;
        let mut na = 0.0;
        let mut nb = 0.0;
        for (term, f) in da {
            let w = self.idf(term) * (1.0 + f.ln());
            na += w * w;
            if let Some(fb) = db.get(term) {
                dot += w * self.idf(term) * (1.0 + fb.ln());
            }
        }
        for (term, f) in db {
            let w = self.idf(term) * (1.0 + f.ln());
            nb += w * w;
        }
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb).sqrt()
        }
    }
}

// ── Command palette ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PaletteCommand {
    pub id: &'static str,
    pub title: String,
    pub hint: String,
    pub group: String,
    pub keywords: Vec<String>,
    pub shortcut: Option<String>,
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .replace('ё', "е")
        .replace(['⌘', '⌥', '⇧', '⌃'], "")
        .trim()
        .to_string()
}

fn acronym(title: &str) -> String {
    normalize(title)
        .split([' ', '-', '_', '/', '·', '—', '(', ')'])
        .filter(|word| !word.is_empty())
        .filter_map(|word| word.chars().next())
        .collect()
}

/// 0 means "does not match"; every query token must occur somewhere.
pub fn score_command(query: &str, command: &PaletteCommand) -> i32 {
    let q = normalize(query);
    if q.is_empty() {
        return 1;
    }
    let title = normalize(&command.title);
    let keywords = normalize(&command.keywords.join(" "));
    let group = normalize(&command.group);
    let hint = normalize(&command.hint);
    let haystack = format!("{title} {keywords} {group} {hint}");
    let tokens: Vec<&str> = q.split_whitespace().filter(|t| !t.is_empty()).collect();
    let compact = q.split_whitespace().collect::<String>();
    let acr = acronym(&command.title);
    let acronym_hit = !compact.is_empty() && acr.starts_with(&compact);
    if !tokens.iter().all(|token| haystack.contains(token)) && !acronym_hit {
        return 0;
    }

    let mut score = 0;
    for token in tokens {
        if title == token {
            score += 70;
        } else if title.starts_with(token) {
            score += 55;
        } else if title
            .split([' ', '-', '_', '/', '·', '—'])
            .any(|word| word.starts_with(token))
        {
            score += 40;
        } else if title.contains(token) {
            score += 25;
        } else if keywords.starts_with(token) {
            score += 22;
        } else if keywords.contains(token) {
            score += 14;
        } else if group.contains(token) {
            score += 8;
        } else {
            score += 4;
        }
    }
    if title.starts_with(&q) {
        score += 30;
    }
    if acronym_hit {
        score += 35;
    }
    score
}

pub fn rank_commands<'a>(
    query: &str,
    commands: &'a [PaletteCommand],
) -> Vec<(&'a PaletteCommand, i32)> {
    let mut ranked: Vec<(&PaletteCommand, i32)> = commands
        .iter()
        .map(|command| (command, score_command(query, command)))
        .filter(|(_, score)| *score > 0)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.title.cmp(&b.0.title)));
    ranked
}

/// Split text into highlighted/normal segments for every query token.
pub fn highlight_matches(text: &str, query: &str) -> Vec<(String, bool)> {
    let tokens: Vec<String> = normalize(query)
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect();
    if text.is_empty() || tokens.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let lower = text.to_lowercase();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for token in &tokens {
        let mut from = 0;
        while let Some(index) = lower[from..].find(token.as_str()) {
            let start = from + index;
            let end = start + token.len();
            ranges.push((start, end));
            from = end.max(start + 1);
            if from >= lower.len() {
                break;
            }
        }
    }
    if ranges.is_empty() {
        return vec![(text.to_string(), false)];
    }
    ranges.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.0 <= last.1 => last.1 = last.1.max(range.1),
            _ => merged.push(range),
        }
    }
    let mut out = Vec::new();
    let mut cursor = 0;
    for (start, end) in merged {
        if start > cursor {
            out.push((text[cursor..start].to_string(), false));
        }
        out.push((text[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < text.len() {
        out.push((text[cursor..].to_string(), false));
    }
    out
}

/// One-line excerpt around the first query match, for the sidebar.
///
/// Returns `None` when the page only matched through tags or the subject: a
/// fabricated excerpt would be worse than none.
pub fn snippet(text: &str, query: &str, radius: usize) -> Option<String> {
    let tokens: Vec<String> = normalize(query)
        .split_whitespace()
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_string)
        .collect();
    if tokens.is_empty() || text.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let mut hit: Option<(usize, usize)> = None;
    for token in &tokens {
        if let Some(position) = lower.find(token.as_str()) {
            let end = position + token.len();
            hit = Some(match hit {
                Some((start, stop)) if position >= start && end <= stop => (start, stop),
                Some((start, stop)) => (start.min(position), stop.max(end)),
                None => (position, end),
            });
        }
    }
    let (start, end) = hit?;
    // `to_lowercase` can change byte lengths, so map back through character
    // counts before slicing the original text.
    let start_char = lower[..start].chars().count();
    let end_char = start_char + lower[start..end].chars().count();
    let chars: Vec<char> = text.chars().collect();
    let from = start_char.saturating_sub(radius);
    let to = (end_char + radius).min(chars.len());
    if from >= to {
        return None;
    }
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.push_str(chars[from..to].iter().collect::<String>().trim());
    if to < chars.len() {
        out.push('…');
    }
    // Page text contains newlines; a sidebar row is one line.
    Some(out.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Rank other documents by tf-idf cosine similarity to `current`.
///
/// `ids` are document ids in caller order, so the returned index points straight
/// back at the caller's slice. Self-matches and weak similarities are dropped;
/// this is "related by meaning" without a model, exactly like the web engine.
pub fn related(
    index: &Bm25Index,
    current: &str,
    ids: &[&str],
    min_score: f32,
    limit: usize,
) -> Vec<(usize, f32)> {
    let mut scored: Vec<(usize, f32)> = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id != current)
        .map(|(position, id)| (position, index.similarity(current, id)))
        .filter(|(_, score)| *score > 0.0 && *score >= min_score)
        .collect();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(limit);
    scored
}

/// Simple subject/tag boost applied on top of BM25.
pub fn subject_boost(query: &str, subject: &str, tags: &[String]) -> f32 {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 0.0;
    }
    let mut boost = 0.0;
    if subject.to_lowercase().contains(&q) {
        boost += 2.5;
    }
    if tags.iter().any(|tag| tag.to_lowercase().contains(&q)) {
        boost += 1.5;
    }
    boost
}

/// Cheap full-text haystack for one capsule, used to re-check BM25 hits.
pub fn capsule_tokens(
    subject: &str,
    tags: &[String],
    blocks: &str,
    attachments: &str,
) -> Vec<String> {
    let mut text = String::with_capacity(subject.len() + blocks.len() + 64);
    text.push_str(subject);
    text.push(' ');
    for tag in tags {
        text.push_str(tag);
        text.push(' ');
    }
    text.push_str(blocks);
    text.push(' ');
    text.push_str(attachments);
    tokenize(&text)
}

/// Tag frequency map for the sidebar/graph filters.
pub fn tag_counts<'a>(tags: impl Iterator<Item = &'a String>) -> BTreeMap<&'a str, usize> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for tag in tags {
        *counts.entry(tag.as_str()).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Bm25Index {
        let mut index = Bm25Index::default();
        index.add("a", tokenize("mmap arena allocator zero copy vault"));
        index.add(
            "b",
            tokenize("allocator latency budget and mmap experiments"),
        );
        index.add("c", tokenize("цветы и кофе"));
        index
    }

    #[test]
    fn tokenizes_latin_and_cyrillic() {
        assert_eq!(
            tokenize("Привет, mmap-vault! ok"),
            vec!["привет", "mmap-vault", "ok"]
        );
    }

    #[test]
    fn ranks_by_relevance() {
        let hits = index().rank("allocator");
        assert_eq!(hits.len(), 2);
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn prefix_matching_works() {
        let hits = index().rank("alloc");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn unknown_terms_return_nothing() {
        assert!(index().rank("квантовый").is_empty());
    }

    #[test]
    fn similarity_is_bounded() {
        let index = index();
        let a = index.similarity("a", "b");
        assert!(a > 0.0 && a <= 1.0);
        assert_eq!(index.similarity("a", "c"), 0.0);
    }

    #[test]
    fn command_scores_by_title_and_acronym() {
        let command = PaletteCommand {
            id: "planning",
            title: "Planning".into(),
            hint: "Agenda, Kanban, Gantt".into(),
            group: "Navigation".into(),
            keywords: vec!["задачи".into(), "tasks".into()],
            shortcut: None,
        };
        assert!(score_command("plan", &command) > 0);
        assert!(score_command("задачи", &command) > 0);
        assert_eq!(score_command("кофе", &command), 0);
    }

    #[test]
    fn highlight_marks_tokens() {
        let segments = highlight_matches("Planning tasks", "plan");
        assert_eq!(segments[0], ("Plan".to_string(), true));
        assert_eq!(segments.last().unwrap().1, false);
    }

    #[test]
    fn snippet_centres_on_the_match() {
        let text = "The reader maps the file and parses headers lazily, so a ten-thousand page vault opens without a database.";
        let snippet = snippet(text, "headers", 12).expect("match");
        assert!(snippet.contains("headers"));
        assert!(snippet.starts_with('…'));
        assert!(snippet.ends_with('…'));
    }

    #[test]
    fn snippet_handles_cyrillic_case_folding() {
        let text = "Память отображается в адресное пространство, заголовки разбираются лениво.";
        let snippet = snippet(text, "ЗАГОЛОВКИ", 8).expect("match");
        assert!(snippet.contains("заголовки"));
    }

    #[test]
    fn snippet_is_none_without_a_match() {
        assert!(snippet("nothing here", "absent", 10).is_none());
        assert!(snippet("", "x", 10).is_none());
    }

    fn related_index() -> Bm25Index {
        let mut index = Bm25Index::default();
        index.add("a", tokenize("mmap arena allocator zero copy vault"));
        index.add(
            "b",
            tokenize("allocator latency budget and mmap experiments"),
        );
        index.add("c", tokenize("цветы и кофе"));
        index
    }

    #[test]
    fn related_ranks_neighbours_and_skips_self() {
        let index = related_index();
        let ids = ["a", "b", "c"];
        let hits = related(&index, "a", &ids, 0.05, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 1);
        assert!(hits[0].1 > 0.0);
    }

    #[test]
    fn related_respects_threshold_and_limit() {
        let index = related_index();
        let ids = ["a", "b", "c"];
        assert!(related(&index, "a", &ids, 0.99, 5).is_empty());
        assert!(related(&index, "a", &ids, 0.0, 0).is_empty());
        let isolated = related(&index, "c", &ids, 0.0, 5);
        assert!(isolated.is_empty(), "no shared terms means no neighbours");
    }
}
