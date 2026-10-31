//! Co-Thinker: deterministic offline cognitive thought linter for CapsuleCraft.
//!
//! Evaluates capsule text, numeric claims, phrasing risks, and CSV data
//! with zero network calls and full determinism.

use crate::i18n::Locale;
use crate::mime::new_block_id;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoThinkMode {
    Critique,
    Synthesis,
    CounterArg,
    DataInsight,
}

#[derive(Clone, Debug)]
pub enum CoThinkOutcome {
    OpponentBlock(Value),
    CalloutBlock(Value),
    ReplyCapsule {
        subject: String,
        body: String,
        blocks: Vec<Value>,
    },
}

#[derive(Clone, Debug)]
pub struct RiskFinding {
    pub rule: &'static str,
    pub title: String,
    pub detail: String,
    pub quote: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PhrasingReport {
    pub vague_words: Vec<String>,
    pub absolute_words: Vec<String>,
    pub long_sentences: usize,
    pub action_items_found: usize,
}

#[derive(Clone, Debug)]
pub struct CsvSeriesStat {
    pub column: String,
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub delta_pct: f64,
    pub trend: &'static str,
}

/// Detects text language by Cyrillic vs Latin frequency.
pub fn detect_lang(text: &str) -> Locale {
    let mut cyr = 0;
    let mut lat = 0;
    for c in text.chars() {
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyr += 1;
        } else if c.is_ascii_alphabetic() {
            lat += 1;
        }
    }
    if cyr >= lat && cyr > 0 {
        Locale::Ru
    } else {
        Locale::En
    }
}

/// Extract top keywords by frequency, filtering basic stopwords.
pub fn extract_keywords(text: &str, limit: usize) -> Vec<String> {
    use std::collections::HashMap;

    let stop: &[&str] = &[
        "и",
        "в",
        "во",
        "не",
        "что",
        "он",
        "на",
        "я",
        "с",
        "со",
        "как",
        "а",
        "то",
        "все",
        "она",
        "так",
        "его",
        "но",
        "да",
        "ты",
        "к",
        "у",
        "же",
        "вы",
        "за",
        "бы",
        "по",
        "только",
        "ее",
        "мне",
        "было",
        "вот",
        "от",
        "меня",
        "еще",
        "нет",
        "о",
        "из",
        "ему",
        "теперь",
        "когда",
        "даже",
        "ну",
        "если",
        "уже",
        "или",
        "ни",
        "быть",
        "был",
        "до",
        "вас",
        "там",
        "потом",
        "себя",
        "может",
        "они",
        "где",
        "есть",
        "надо",
        "для",
        "мы",
        "тебя",
        "их",
        "чем",
        "это",
        "the",
        "of",
        "to",
        "and",
        "a",
        "in",
        "that",
        "is",
        "was",
        "for",
        "it",
        "with",
        "as",
        "on",
        "be",
        "at",
        "by",
        "this",
        "had",
        "not",
        "are",
        "but",
        "from",
        "or",
        "have",
        "an",
        "they",
        "which",
        "one",
        "you",
        "were",
        "her",
        "all",
        "there",
        "would",
        "their",
        "we",
    ];

    let mut freqs: HashMap<String, usize> = HashMap::new();
    let words = text.split(|c: char| !c.is_alphanumeric() && c != '_');

    for raw in words {
        let w = raw.trim().to_lowercase();
        if w.chars().count() < 3 || w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if stop.contains(&w.as_str()) {
            continue;
        }
        *freqs.entry(w).or_insert(0) += 1;
    }

    let mut sorted: Vec<(String, usize)> = freqs.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.into_iter().take(limit).map(|(w, _)| w).collect()
}

/// Finds sentences containing concrete numbers, currencies, or percentages.
pub fn find_claims(text: &str, limit: usize) -> Vec<String> {
    let mut claims = Vec::new();
    for line in text.lines() {
        for s in line.split(|c| c == '.' || c == '!' || c == '?' || c == ';') {
            let tr = s.trim();
            if tr.chars().count() > 12 && tr.chars().any(|c| c.is_ascii_digit()) {
                claims.push(tr.to_string());
            }
        }
    }

    claims.sort_by(|a, b| {
        let score = |s: &str| -> i32 {
            let mut sc = 0;
            if s.contains('%') || s.contains('$') || s.contains('₽') || s.contains('€') {
                sc += 3;
            }
            if s.contains('.') || s.contains(',') {
                sc += 1;
            }
            sc
        };
        score(b).cmp(&score(a))
    });

    claims.truncate(limit);
    claims
}

/// Detects cognitive risks: absolute terms, unbuffered deadlines, lack of fallback.
pub fn detect_risks(text: &str, data_points: usize, lang: Locale) -> Vec<RiskFinding> {
    let lower = text.to_lowercase();
    let mut risks = Vec::new();

    let abs_words = if lang == Locale::Ru {
        &[
            "всегда",
            "никогда",
            "100%",
            "гарантированно",
            "абсолютно",
            "точно",
            "без сомнений",
        ][..]
    } else {
        &[
            "always",
            "never",
            "100%",
            "guaranteed",
            "absolutely",
            "definitely",
            "without doubt",
        ][..]
    };

    for w in abs_words {
        if lower.contains(w) {
            risks.push(RiskFinding {
                rule: "absolute_phrasing",
                title: if lang == Locale::Ru {
                    "Категоричное утверждение".into()
                } else {
                    "Absolute phrasing".into()
                },
                detail: if lang == Locale::Ru {
                    format!("Использовано безапелляционное слово «{}». Реальные сценарии требуют учета краевых условий.", w)
                } else {
                    format!("Absolute term '{}' found. Edge cases require contingency margins.", w)
                },
                quote: Some(w.to_string()),
            });
            break;
        }
    }

    let has_safety = if lang == Locale::Ru {
        lower.contains("риск")
            || lower.contains("буфер")
            || lower.contains("резерв")
            || lower.contains("fallback")
    } else {
        lower.contains("risk")
            || lower.contains("buffer")
            || lower.contains("reserve")
            || lower.contains("fallback")
    };

    if !has_safety && text.len() > 100 {
        risks.push(RiskFinding {
            rule: "no_downside_defense",
            title: if lang == Locale::Ru {
                "Отсутствие буфера безопасности".into()
            } else {
                "No downside protection".into()
            },
            detail: if lang == Locale::Ru {
                "В тексте отсутствуют упоминания рисков, запасных планов (fallback) или резервных буферов.".into()
            } else {
                "No mention of risks, fallbacks, or safety buffers found in the content.".into()
            },
            quote: None,
        });
    }

    if data_points > 0 && data_points < 5 {
        risks.push(RiskFinding {
            rule: "thin_extrapolation",
            title: if lang == Locale::Ru {
                "Мало данных для экстраполяции".into()
            } else {
                "Thin data points".into()
            },
            detail: if lang == Locale::Ru {
                format!(
                    "В таблице всего {} точек данных. Тренды и выводы статистически неустойчивы.",
                    data_points
                )
            } else {
                format!(
                    "Only {} data points detected. Predictions are statistically fragile.",
                    data_points
                )
            },
            quote: None,
        });
    }

    risks
}

/// Cognitive linter for vague words and readability.
pub fn inspect_phrasing(text: &str, lang: Locale) -> PhrasingReport {
    let lower = text.to_lowercase();
    let mut rep = PhrasingReport::default();

    let vague_list = if lang == Locale::Ru {
        &[
            "скоро",
            "быстро",
            "лучше",
            "оптимизировать",
            "гибкий",
            "проще",
            "эффективнее",
        ][..]
    } else {
        &[
            "soon",
            "quickly",
            "better",
            "optimize",
            "flexible",
            "easier",
            "efficient",
        ][..]
    };

    for w in vague_list {
        if lower.contains(w) {
            rep.vague_words.push(w.to_string());
        }
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ]")
            || trimmed.starts_with("- [x]")
            || trimmed.starts_with("TODO:")
        {
            rep.action_items_found += 1;
        }
        let words_cnt = trimmed.split_whitespace().count();
        if words_cnt > 35 {
            rep.long_sentences += 1;
        }
    }

    rep
}

/// Parses simple numeric columns from CSV and calculates basic stats & OLS trend.
pub fn analyze_csv(csv: &str) -> Vec<CsvSeriesStat> {
    let mut lines = csv.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
    let header_line = match lines.next() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let headers: Vec<String> = header_line
        .split(',')
        .map(|s| s.trim_matches('"').trim().to_string())
        .collect();
    if headers.is_empty() {
        return Vec::new();
    }

    let mut numeric_data: Vec<Vec<f64>> = vec![Vec::new(); headers.len()];

    for line in lines {
        let parts: Vec<&str> = line.split(',').collect();
        for (i, p) in parts.iter().enumerate() {
            if i < numeric_data.len() {
                let cleaned = p
                    .trim_matches('"')
                    .trim()
                    .replace('$', "")
                    .replace('€', "")
                    .replace('%', "");
                if let Ok(v) = cleaned.parse::<f64>() {
                    numeric_data[i].push(v);
                }
            }
        }
    }

    let mut stats = Vec::new();
    for (i, series) in numeric_data.into_iter().enumerate() {
        if series.len() >= 2 {
            let n = series.len();
            let min = series.iter().copied().fold(f64::INFINITY, f64::min);
            let max = series.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let sum: f64 = series.iter().sum();
            let mean = sum / (n as f64);

            let first = series[0];
            let last = series[n - 1];
            let delta_pct = if first != 0.0 {
                ((last - first) / first.abs()) * 100.0
            } else {
                0.0
            };

            // OLS slope
            let x_mean = (n - 1) as f64 / 2.0;
            let mut num = 0.0;
            let mut den = 0.0;
            for (idx, &y) in series.iter().enumerate() {
                let x = idx as f64;
                num += (x - x_mean) * (y - mean);
                den += (x - x_mean) * (x - x_mean);
            }
            let slope = if den.abs() > 1e-9 { num / den } else { 0.0 };

            let trend = if slope > 0.01 {
                "Up"
            } else if slope < -0.01 {
                "Down"
            } else {
                "Flat"
            };

            stats.push(CsvSeriesStat {
                column: headers
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| format!("Col {}", i + 1)),
                count: n,
                min,
                max,
                mean,
                delta_pct,
                trend,
            });
        }
    }

    stats
}

/// Extracts full text from JSON capsule blocks.
pub fn extract_capsule_text(subject: &str, blocks: &[Value]) -> String {
    let mut out = String::new();
    if !subject.is_empty() {
        out.push_str(subject);
        out.push('\n');
    }
    for b in blocks {
        let d = &b["data"];
        if let Some(t) = d["text"].as_str() {
            out.push_str(t);
            out.push('\n');
        }
        if let Some(t) = d["content"].as_str() {
            out.push_str(t);
            out.push('\n');
        }
        if let Some(t) = d["title"].as_str() {
            out.push_str(t);
            out.push('\n');
        }
    }
    out
}

/// Generates Co-Think analysis outcome based on selected mode.
pub fn generate_cothink(
    mode: CoThinkMode,
    subject: &str,
    blocks: &[Value],
    csv_attachments: &[(&str, &str)],
    focus_prompt: &str,
) -> CoThinkOutcome {
    let text = extract_capsule_text(subject, blocks);
    let lang = detect_lang(&text);
    let kw = extract_keywords(&text, 6);
    let claims = find_claims(&text, 3);
    let risks = detect_risks(&text, csv_attachments.len(), lang);
    let phrasing = inspect_phrasing(&text, lang);

    match mode {
        CoThinkMode::Critique => {
            let mut crit = String::new();
            if !focus_prompt.trim().is_empty() {
                crit.push_str(if lang == Locale::Ru {
                    "🎯 **Фокус анализа:** "
                } else {
                    "🎯 **Analysis Focus:** "
                });
                crit.push_str(focus_prompt.trim());
                crit.push_str("\n\n");
            }

            if !claims.is_empty() {
                crit.push_str(if lang == Locale::Ru {
                    "⚡ **Проверка ключевых утверждений:**\n"
                } else {
                    "⚡ **Key Claims Scrutiny:**\n"
                });
                for c in &claims {
                    if lang == Locale::Ru {
                        crit.push_str(&format!(
                            "• «{}» — на чем основана эта цифра? Заложен ли буфер на отклонение?\n",
                            c
                        ));
                    } else {
                        crit.push_str(&format!("• “{}” — what is this figure based on? Is there a buffer for deviation?\n", c));
                    }
                }
                crit.push('\n');
            }

            if !risks.is_empty() {
                crit.push_str(if lang == Locale::Ru {
                    "⚠️ **Выявленные зоны риска:**\n"
                } else {
                    "⚠️ **Identified Risk Areas:**\n"
                });
                for r in &risks {
                    crit.push_str(&format!("• **{}**: {}\n", r.title, r.detail));
                }
                crit.push('\n');
            }

            if !phrasing.vague_words.is_empty() {
                crit.push_str(if lang == Locale::Ru {
                    "🔍 **Когнитивный линтер:** обнаружены размытые формулировки ("
                } else {
                    "🔍 **Thought Linter:** vague phrasing detected ("
                });
                crit.push_str(&phrasing.vague_words.join(", "));
                crit.push_str(if lang == Locale::Ru {
                    "). Рекомендуется конкретизировать критерии успеха.\n"
                } else {
                    "). Concrete acceptance criteria recommended.\n"
                });
            }

            if crit.is_empty() {
                crit.push_str(if lang == Locale::Ru {
                    "Текст сформулирован компактно. Добавьте больше деталей, числовых метрик или задач для глубокой проверки."
                } else {
                    "The text is concise. Provide more details, numerical targets, or tasks for deeper scrutiny."
                });
            }

            let source_claim = claims.first().cloned().unwrap_or_else(|| {
                if !kw.is_empty() {
                    kw.join(", ")
                } else {
                    subject.to_string()
                }
            });

            CoThinkOutcome::OpponentBlock(json!({
                "id": new_block_id("ai_opponent_block"),
                "type": "ai_opponent_block",
                "data": {
                    "criticism": crit.trim(),
                    "sourceClaim": source_claim,
                    "confidence": 0.85
                }
            }))
        }

        CoThinkMode::Synthesis => {
            let mut synth = String::new();
            synth.push_str(if lang == Locale::Ru {
                "### Ключевая суть и контекст\n"
            } else {
                "### Core Essence & Context\n"
            });
            if !kw.is_empty() {
                let topics_label = if lang == Locale::Ru {
                    "Темы"
                } else {
                    "Topics"
                };
                synth.push_str(&format!("**{}:** {}\n\n", topics_label, kw.join(" · ")));
            }

            synth.push_str(if lang == Locale::Ru {
                "### Практические следующие шаги\n"
            } else {
                "### Concrete Next Steps\n"
            });
            if phrasing.action_items_found > 0 {
                synth.push_str(if lang == Locale::Ru {
                    "В документе уже есть задачи. Рекомендуется зафиксировать владельцев и точные сроки проверки промежуточных результатов.\n"
                } else {
                    "Action items are present. Ensure each item has a dedicated owner and verification milestone.\n"
                });
            } else {
                synth.push_str(if lang == Locale::Ru {
                    "1. Зафиксировать конкретный измеримый результат первого этапа.\n2. Определить граничные условия и точки отказа (fallback).\n3. Сформулировать четкий критерий завершения (Definition of Done).\n"
                } else {
                    "1. Establish a clear, measurable outcome for milestone one.\n2. Identify boundary conditions and failure fallbacks.\n3. Formulate a crisp Definition of Done.\n"
                });
            }

            CoThinkOutcome::CalloutBlock(json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "data": {
                    "title": if lang == Locale::Ru { "Синтез Co-Thinker" } else { "AI Synthesis (Co-Thinker)" },
                    "content": synth.trim(),
                    "icon": "check",
                    "tone": "emerald"
                }
            }))
        }

        CoThinkMode::DataInsight => {
            let mut content = String::new();
            if csv_attachments.is_empty() {
                content.push_str(if lang == Locale::Ru {
                    "В заметке не обнаружено вложенных CSV-таблиц. Прикрепите датасет в формате CSV для построения честного OLS-тренда, корреляций и статистического анализа."
                } else {
                    "No CSV attachments found in this capsule. Attach a CSV dataset to get honest OLS regression trends and descriptive statistics."
                });
            } else {
                for (name, csv_data) in csv_attachments {
                    if lang == Locale::Ru {
                        content.push_str(&format!("#### Датасет: {}\n", name));
                    } else {
                        content.push_str(&format!("#### Dataset: {}\n", name));
                    }
                    let stats = analyze_csv(csv_data);
                    if stats.is_empty() {
                        content.push_str(if lang == Locale::Ru {
                            "• Не удалось обнаружить числовые колонки для анализа.\n"
                        } else {
                            "• No numeric columns could be parsed for analysis.\n"
                        });
                    } else {
                        for s in &stats {
                            if lang == Locale::Ru {
                                content.push_str(&format!(
                                    "• **{}** (N={}): Среднее={:.2}, Мин={:.2}, Макс={:.2}, Динамика={:+.1}%, Тренд={}\n",
                                    s.column, s.count, s.mean, s.min, s.max, s.delta_pct, s.trend
                                ));
                            } else {
                                content.push_str(&format!(
                                    "• **{}** (N={}): Mean={:.2}, Min={:.2}, Max={:.2}, Change={:+.1}%, Trend={}\n",
                                    s.column, s.count, s.mean, s.min, s.max, s.delta_pct, s.trend
                                ));
                            }
                        }
                    }
                    content.push('\n');
                }
            }

            CoThinkOutcome::CalloutBlock(json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "data": {
                    "title": if lang == Locale::Ru { "Инсайт по данным (Co-Thinker)" } else { "Data Insight (Co-Thinker)" },
                    "content": content.trim(),
                    "icon": "info",
                    "tone": "blue"
                }
            }))
        }

        CoThinkMode::CounterArg => {
            let reply_sub = format!(
                "Re: {} (Co-Thinker)",
                if subject.is_empty() {
                    "Capsule"
                } else {
                    subject
                }
            );
            let mut body = String::new();
            body.push_str(if lang == Locale::Ru {
                "## Альтернативный взгляд и контраргументы\n\n"
            } else {
                "## Adversarial Perspective & Counter-Arguments\n\n"
            });

            if !claims.is_empty() {
                body.push_str(if lang == Locale::Ru {
                    "### Оспаривание исходных предпосылок:\n"
                } else {
                    "### Challenging the Initial Premises:\n"
                });
                for c in &claims {
                    body.push_str(&format!("> {}\n\n", c));
                    body.push_str(if lang == Locale::Ru {
                        "Что произойдет, если внешние условия изменятся или конверсия упадет вдвое? Данная оценка выглядит сверхуверенной без стресс-тестирования.\n\n"
                    } else {
                        "What happens if conditions shift or performance drops by 50%? This projection appears overly optimistic without stress-testing.\n\n"
                    });
                }
            } else {
                body.push_str(if lang == Locale::Ru {
                    "Основной риск текущей постановки — недостаток формализованных критериев проверки гипотезы. Что послужит опровержением идеи?\n\n"
                } else {
                    "The core vulnerability is the lack of falsifiability criteria. What concrete finding would disprove this thesis?\n\n"
                });
            }

            let blocks = vec![json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": body.trim() }
            })];

            CoThinkOutcome::ReplyCapsule {
                subject: reply_sub,
                body: body.trim().to_string(),
                blocks,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lang_detect() {
        assert_eq!(
            detect_lang("Привет, мир! Это тестовая страница"),
            Locale::Ru
        );
        assert_eq!(detect_lang("Hello world, this is English text"), Locale::En);
    }

    #[test]
    fn test_extract_keywords() {
        let text = "проект архитектура система проект сервис архитектура проект";
        let kw = extract_keywords(text, 3);
        assert_eq!(kw.first().map(|s| s.as_str()), Some("проект"));
    }

    #[test]
    fn test_find_claims() {
        let text = "Мы выросли на 45% за прошлый квартал! Обычное предложение без цифр. Бюджет составил $12000.";
        let claims = find_claims(text, 2);
        assert_eq!(claims.len(), 2);
        assert!(claims[0].contains('%') || claims[0].contains('$'));
    }

    #[test]
    fn test_csv_analysis() {
        let csv = "Day,Users,Revenue\n1,100,500.0\n2,120,600.0\n3,150,750.0";
        let stats = analyze_csv(csv);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[1].column, "Users");
        assert_eq!(stats[1].trend, "Up");
    }

    #[test]
    fn test_generate_cothink_modes() {
        let blocks = vec![json!({
            "id": "p1",
            "type": "paragraph",
            "data": { "text": "План запуска продукта: мы гарантированно вырастем на 50% без риска." }
        })];

        // 1. Critique
        let crit = generate_cothink(CoThinkMode::Critique, "Запуск", &blocks, &[], "бюджет");
        match crit {
            CoThinkOutcome::OpponentBlock(b) => {
                assert_eq!(b["type"], "ai_opponent_block");
                let c = b["data"]["criticism"].as_str().unwrap();
                assert!(c.contains("Фокус анализа"));
                assert!(c.contains("бюджет"));
            }
            _ => panic!("Expected OpponentBlock"),
        }

        // 2. Synthesis
        let synth = generate_cothink(CoThinkMode::Synthesis, "Запуск", &blocks, &[], "");
        match synth {
            CoThinkOutcome::CalloutBlock(b) => {
                assert_eq!(b["type"], "callout");
                assert_eq!(b["data"]["tone"], "emerald");
            }
            _ => panic!("Expected CalloutBlock"),
        }

        // 3. CounterArg
        let counter = generate_cothink(CoThinkMode::CounterArg, "Запуск", &blocks, &[], "");
        match counter {
            CoThinkOutcome::ReplyCapsule {
                subject, blocks, ..
            } => {
                assert_eq!(subject, "Re: Запуск (Co-Thinker)");
                assert!(!blocks.is_empty());
            }
            _ => panic!("Expected ReplyCapsule"),
        }

        // 4. DataInsight
        let csv = "Day,Sales\n1,10\n2,20";
        let data = generate_cothink(
            CoThinkMode::DataInsight,
            "Запуск",
            &blocks,
            &[("sales.csv", csv)],
            "",
        );
        match data {
            CoThinkOutcome::CalloutBlock(b) => {
                assert_eq!(b["type"], "callout");
                assert_eq!(b["data"]["tone"], "blue");
                let text = b["data"]["content"].as_str().unwrap();
                assert!(text.contains("Sales"));
                assert!(text.contains("Датасет"));
            }
            _ => panic!("Expected CalloutBlock"),
        }
    }

    #[test]
    fn test_generate_cothink_english_output() {
        // English capsule → every generated string must be Latin-only.
        let blocks = vec![json!({
            "id": "p1",
            "type": "paragraph",
            "data": { "text": "Launch plan: we will guaranteed grow by 50% without any risk review. Revenue projection is 12000 dollars soon." }
        })];
        let has_cyrillic = |s: &str| s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));

        let crit = generate_cothink(CoThinkMode::Critique, "Launch", &blocks, &[], "budget");
        match crit {
            CoThinkOutcome::OpponentBlock(b) => {
                let c = b["data"]["criticism"].as_str().unwrap();
                assert!(c.contains("Analysis Focus"));
                assert!(c.contains("budget"));
                assert!(!has_cyrillic(c), "criticism leaked Cyrillic: {}", c);
            }
            _ => panic!("Expected OpponentBlock"),
        }

        let synth = generate_cothink(CoThinkMode::Synthesis, "Launch", &blocks, &[], "");
        match synth {
            CoThinkOutcome::CalloutBlock(b) => {
                let c = b["data"]["content"].as_str().unwrap();
                assert!(c.contains("Core Essence"));
                assert!(!has_cyrillic(c), "synthesis leaked Cyrillic: {}", c);
            }
            _ => panic!("Expected CalloutBlock"),
        }

        let counter = generate_cothink(CoThinkMode::CounterArg, "Launch", &blocks, &[], "");
        match counter {
            CoThinkOutcome::ReplyCapsule { subject, body, .. } => {
                assert_eq!(subject, "Re: Launch (Co-Thinker)");
                assert!(!has_cyrillic(&body), "counter leaked Cyrillic: {}", body);
            }
            _ => panic!("Expected ReplyCapsule"),
        }

        let csv = "Day,Sales\n1,10\n2,20\n3,35";
        let data = generate_cothink(
            CoThinkMode::DataInsight,
            "Launch",
            &blocks,
            &[("sales.csv", csv)],
            "",
        );
        match data {
            CoThinkOutcome::CalloutBlock(b) => {
                let c = b["data"]["content"].as_str().unwrap();
                assert!(c.contains("Dataset"));
                assert!(c.contains("Mean"));
                assert!(!has_cyrillic(c), "data insight leaked Cyrillic: {}", c);
            }
            _ => panic!("Expected CalloutBlock"),
        }
    }
}
