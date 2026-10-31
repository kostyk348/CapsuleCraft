//! Deterministic offline JavaScript execution through the boa engine
//! (roadmap item: code execution in the native shell).
//!
//! Two entry points:
//! - [`run_javascript`] — REPL semantics for code blocks: a `console` shim
//!   collects `log/warn/error`, the completion value is rendered like the web
//!   worker does (`logs… \n=> result`).
//! - [`eval_formula`] — spreadsheet-style `=…` cells for data grids: cell
//!   references (`A1`) are substituted with their numeric values, range
//!   arguments (`SUM(A1:A5)`) are expanded, and the expression is evaluated
//!   with the same engine, so both features share one interpreter.

use boa_engine::{Context, Source};

/// Console shim mirrored from the web worker implementation.
const CONSOLE_SHIM: &str = r#"
var __logs = [];
var console = {
  log: function() {
    var a = Array.prototype.slice.call(arguments).map(function (x) {
      try {
        return (typeof x === 'object' && x !== null) ? JSON.stringify(x, null, 2) : String(x);
      } catch (_) { return String(x); }
    });
    __logs.push(a.join(' '));
  },
  warn: function() { __logs.push('WARN: ' + Array.prototype.slice.call(arguments).join(' ')); },
  error: function() { __logs.push('ERROR: ' + Array.prototype.slice.call(arguments).join(' ')); }
};
"#;

/// Aggregate helpers available inside `=…` formulas.
const FORMULA_PRELUDE: &str = r#"
function SUM() { return Array.prototype.slice.call(arguments).reduce(function (a, b) { return a + Number(b); }, 0); }
function AVG() { var a = Array.prototype.slice.call(arguments); return a.length ? SUM.apply(null, a) / a.length : NaN; }
function MIN() { return Math.min.apply(null, arguments); }
function MAX() { return Math.max.apply(null, arguments); }
function COUNT() { return arguments.length; }
"#;

/// Run user code with a console shim; returns the same text layout the web
/// REPL block produces: captured logs, then `=> <completion value>`.
pub fn run_javascript(code: &str) -> Result<String, String> {
    let mut context = Context::default();
    context
        .eval(Source::from_bytes(CONSOLE_SHIM))
        .map_err(|err| err.to_string())?;

    let value = context.eval(Source::from_bytes(code));
    let logs = context
        .eval(Source::from_bytes("__logs.join('\\n')"))
        .map(|v| v.to_string(&mut context).map(|s| s.to_std_string_escaped()).unwrap_or_default())
        .unwrap_or_default();

    match value {
        Ok(value) => {
            let rendered = if value.is_undefined() {
                String::new()
            } else if value.is_object() {
                // Objects render as JSON when possible, like the web worker.
                match value.to_json(&mut context) {
                    Ok(json) => serde_json::to_string_pretty(&json)
                        .unwrap_or_else(|_| value.display().to_string()),
                    Err(_) => value
                        .to_string(&mut context)
                        .map(|s| s.to_std_string_escaped())
                        .unwrap_or_else(|_| "[object]".into()),
                }
            } else {
                value
                    .to_string(&mut context)
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|_| value.display().to_string())
            };
            let mut out = logs;
            if !rendered.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("=> ");
                out.push_str(&rendered);
            }
            Ok(if out.is_empty() {
                "/* Executed successfully (no stdout) */".to_string()
            } else {
                out
            })
        }
        Err(err) => Err(if logs.is_empty() {
            err.to_string()
        } else {
            format!("{logs}\n{err}")
        }),
    }
}

/// Spreadsheet reference: column letter(s) + 1-based row, e.g. `A1`, `BC12`.
fn cell_value(formulas: &[Vec<String>], row: usize, col: usize) -> Option<f64> {
    let raw = formulas.get(row)?.get(col)?.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    let resolved = if let Some(expr) = raw.strip_prefix('=') {
        eval_formula(expr, formulas, 0)?
    } else {
        raw.clone()
    };
    resolved.trim().parse::<f64>().ok()
}

/// Convert a 0-based column index to letters: 0 → A, 25 → Z, 26 → AA.
fn column_name(mut col: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii only")
}

/// Expand `A1:B3` ranges and single refs inside a formula body.
fn expand_refs(expr: &str, formulas: &[Vec<String>], depth: usize) -> Result<String, String> {
    if depth > 8 {
        return Err("formula recursion too deep".into());
    }
    let chars: Vec<char> = expr.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Range or reference: [A-Z]+[0-9]+ optionally followed by ':' and another ref.
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let col_start = start;
            let col_end = i;
            let row_start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i > row_start {
                let col = letters_to_index(&chars[col_start..col_end])?;
                let row: usize = chars[row_start..i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .map_err(|_| format!("bad row in {}", expr))?;
                if row == 0 {
                    return Err("rows are 1-based".into());
                }
                // Range?
                if i + 1 < chars.len() && chars[i] == ':' && chars[i + 1].is_ascii_alphabetic() {
                    i += 1;
                    let r2_col_start = i;
                    while i < chars.len() && chars[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let r2_col_end = i;
                    let r2_row_start = i;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i <= r2_row_start {
                        return Err("bad range end".into());
                    }
                    let col2 = letters_to_index(&chars[r2_col_start..r2_col_end])?;
                    let row2: usize = chars[r2_row_start..i]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .map_err(|_| "bad range row".to_string())?;
                    let (r1, r2) = (row.min(row2), row.max(row2));
                    let (c1, c2) = (col.min(col2), col.max(col2));
                    let mut values = Vec::new();
                    for rr in r1..=r2 {
                        for cc in c1..=c2 {
                            match cell_value(formulas, rr - 1, cc) {
                                Some(v) => values.push(format_number(v)),
                                None => return Err(format!("empty cell {}{}", column_name(cc), rr)),
                            }
                        }
                    }
                    out.push_str(&values.join(", "));
                    continue;
                }
                match cell_value(formulas, row - 1, col) {
                    Some(v) => out.push_str(&format_number(v)),
                    None => return Err(format!("empty cell {}{}", column_name(col), row)),
                }
                continue;
            }
            // A bare word (function name like SUM) — copy verbatim.
            out.push_str(&chars[start..i].iter().collect::<String>());
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn letters_to_index(letters: &[char]) -> Result<usize, String> {
    let mut index = 0usize;
    for ch in letters {
        let upper = ch.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return Err("bad column letter".into());
        }
        index = index * 26 + (upper as usize - 'A' as usize + 1);
    }
    Ok(index.saturating_sub(1))
}

/// Trim float noise the way spreadsheet apps do: `3.0` → `3`.
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Evaluate a `=…` formula against the grid. `depth` guards recursive cells.
pub fn eval_formula(expr: &str, formulas: &[Vec<String>], depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let expanded = expand_refs(expr, formulas, depth).ok()?;
    let mut context = Context::default();
    context.eval(Source::from_bytes(FORMULA_PRELUDE)).ok()?;
    let value = context.eval(Source::from_bytes(expanded.as_str())).ok()?;
    if let Some(num) = value.as_number() {
        if num.is_nan() || num.is_infinite() {
            return None;
        }
        return Some(format_number(num));
    }
    value
        .to_string(&mut context)
        .ok()
        .map(|s| s.to_std_string_escaped())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_console_capture() {
        let out = run_javascript("console.log('hi', 2 + 2); 6 * 7;").expect("runs");
        assert_eq!(out, "hi 4\n=> 42");

        let obj = run_javascript("({ a: 1, b: 'x' })").expect("runs");
        assert!(obj.contains("\"a\": 1"), "objects render as JSON: {obj}");

        let silent = run_javascript("var x = 1;").expect("runs");
        assert_eq!(silent, "/* Executed successfully (no stdout) */");

        let logged = run_javascript("console.warn('careful');").expect("runs");
        assert_eq!(logged, "WARN: careful");
    }

    #[test]
    fn errors_surface_with_logs() {
        let err = run_javascript("console.log('before'); nope();").unwrap_err();
        assert!(err.contains("before"), "logs precede the error: {err}");
        assert!(err.contains("ReferenceError") || err.contains("not defined"));
    }

    #[test]
    fn string_result_renders_without_quotes() {
        let out = run_javascript("'capsule' + 'craft'").expect("runs");
        assert_eq!(out, "=> capsulecraft");
    }

    #[test]
    fn formula_sum_and_avg() {
        let grid = vec![
            vec!["Price".into(), "Qty".into()],
            vec!["10".into(), "2".into()],
            vec!["30".into(), "4".into()],
            vec!["=SUM(A2:A3)".into(), "=AVG(B2:B3)".into()],
        ];
        assert_eq!(eval_formula("SUM(A2:A3)", &grid, 0), Some("40".into()));
        assert_eq!(eval_formula("AVG(B2:B3)", &grid, 0), Some("3".into()));
    }

    #[test]
    fn formula_references_and_nested_formulas() {
        let grid = vec![
            vec!["5".into(), "=A1*2".into()],
            vec!["=B1+1".into(), "=SUM(A1:B1)".into()],
        ];
        assert_eq!(eval_formula("B1+1", &grid, 0), Some("11".into()));
        assert_eq!(eval_formula("SUM(A1:B1)", &grid, 0), Some("15".into()));
    }

    #[test]
    fn formula_errors_are_none() {
        let grid = vec![vec!["1".into()]];
        assert_eq!(eval_formula("A1 + ", &grid, 0), None);
        assert_eq!(eval_formula("B9", &grid, 0), None);
        assert_eq!(eval_formula("ZZZ99", &grid, 0), None);
    }

    #[test]
    fn column_names_match_letters() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(letters_to_index(&['A']), Ok(0));
        assert_eq!(letters_to_index(&['A', 'A']), Ok(26));
    }
}
