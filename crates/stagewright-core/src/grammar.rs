//! Grammar in action and stage directions: Harper (a grammar checker that runs
//! on this computer, so nothing is sent anywhere) plus a few British usages it
//! misses. Dialogue is left alone: characters speak as they speak, and period
//! dialogue would set off false alarms. Spelling is the system spellchecker's job.

use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::parsers::PlainEnglish;
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// Which English the grammar check expects (`dialect:` in project.yaml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum English {
    #[default]
    British,
    American,
    Canadian,
    Australian,
    Indian,
}

impl English {
    fn dialect(self) -> Dialect {
        match self {
            English::British => Dialect::British,
            English::American => Dialect::American,
            English::Canadian => Dialect::Canadian,
            English::Australian => Dialect::Australian,
            English::Indian => Dialect::Indian,
        }
    }
}

/// One finding, in character columns of the text checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub from: usize,
    pub to: usize,
    pub message: String,
    /// What to put in place of `from..to`, when there's a clear fix.
    pub replacement: Option<String>,
}

/// Kinds left out: spelling (the system spellchecker does it, and doesn't flag
/// character names), and readability and style advice (sentence length is the
/// long-action check's business).
const SKIP: &[LintKind] = &[
    LintKind::Spelling,
    LintKind::Readability,
    LintKind::Style,
    LintKind::Enhancement,
    LintKind::Repetition, // the repeated-word check, which knows "had had"
];

/// Harper rules that misfire on scripts: "the front right of whom is JOHN DEE" is right.
const OFF_RULES: &[&str] = &["WhomSubjectOfVerb"];

/// Linters are slow to build (most of a second) and quick to run: one per dialect, kept.
static LINTERS: OnceLock<Mutex<Vec<(English, LintGroup)>>> = OnceLock::new();

fn with_linter<T>(english: English, f: impl FnOnce(&mut LintGroup) -> T) -> T {
    let all = LINTERS.get_or_init(|| Mutex::new(Vec::new()));
    let mut all = all.lock().unwrap_or_else(|e| e.into_inner());
    if !all.iter().any(|(e, _)| *e == english) {
        let mut lg = LintGroup::new_curated(FstDictionary::curated(), english.dialect());
        for rule in OFF_RULES {
            lg.config.set_rule_enabled(*rule, false);
        }
        all.push((english, lg));
    }
    let (_, lg) = all.iter_mut().find(|(e, _)| *e == english).unwrap();
    f(lg)
}

/// Build the linter in the background, so the first check doesn't wait for it.
pub fn warm_up(english: English) {
    std::thread::spawn(move || with_linter(english, |_| ()));
}

/// British usages Harper doesn't flag: "was sat", "him self", "next the".
fn house_rules(text: &str) -> Vec<Finding> {
    let words: Vec<(usize, usize, String)> = {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_alphabetic() {
                let s = i;
                while i < chars.len() && chars[i].is_alphabetic() {
                    i += 1;
                }
                out.push((s, i, chars[s..i].iter().collect::<String>()));
            } else {
                i += 1;
            }
        }
        out
    };
    let chars: Vec<char> = text.chars().collect();
    let spaced =
        |a: &(usize, usize, String), b: &(usize, usize, String)| chars[a.1..b.0].iter().all(|c| *c == ' ') && b.0 > a.1;
    let mut out = Vec::new();
    for w in words.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if !spaced(a, b) {
            continue;
        }
        let (la, lb) = (a.2.to_lowercase(), b.2.to_lowercase());
        // "was sat", "has been stood"
        if ["was", "were", "is", "are", "am", "be", "been", "being"].contains(&la.as_str())
            && let Some(ing) = match lb.as_str() {
                "sat" => Some("sitting"),
                "stood" => Some("standing"),
                _ => None,
            }
        {
            out.push(Finding {
                from: b.0,
                to: b.1,
                message: format!("\"{} {}\" is colloquial; in the action, \"{} {ing}\".", a.2, b.2, a.2),
                replacement: Some(ing.to_string()),
            });
        }
        // "him self" -> "himself"
        let joined = format!("{la}{lb}");
        if [
            "himself",
            "herself",
            "myself",
            "yourself",
            "yourselves",
            "itself",
            "ourselves",
            "themselves",
            "oneself",
        ]
        .contains(&joined.as_str())
        {
            let fixed = format!("{}{}", a.2, b.2);
            out.push(Finding {
                from: a.0,
                to: b.1,
                message: format!("\"{fixed}\" is one word."),
                replacement: Some(fixed),
            });
        }
        // "next the grave" -> "next to the grave"
        if la == "next" && lb == "the" {
            out.push(Finding {
                from: a.0,
                to: b.1,
                message: "\"next the\" usually needs \"to\": \"next to the\".".into(),
                replacement: Some(format!("{} to {}", a.2, b.2)),
            });
        }
    }
    out
}

/// Findings already worked out, by paragraph: most paragraphs don't change between
/// one check and the next (as you type, or when Production checks every scene).
type Cache = std::collections::HashMap<(English, String), Vec<Finding>>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
const CACHE_LIMIT: usize = 20_000;

/// Grammar findings for one paragraph of action or direction.
pub fn check(text: &str, english: English) -> Vec<Finding> {
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let key = (english, text.to_string());
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let found = check_uncached(text, english);
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    if c.len() >= CACHE_LIMIT {
        c.clear();
    }
    c.insert(key, found.clone());
    found
}

fn check_uncached(text: &str, english: English) -> Vec<Finding> {
    let mut out = house_rules(text);
    let lints = with_linter(english, |lg| lg.lint(&Document::new_curated(text, &PlainEnglish)));
    let chars: Vec<char> = text.chars().collect();
    for l in lints {
        if SKIP.contains(&l.lint_kind) || l.span.end > chars.len() || l.span.start >= l.span.end {
            continue;
        }
        // Words in capitals are names, places and camera terms: not grammar.
        let span: String = chars[l.span.start..l.span.end].iter().collect();
        if span.chars().any(char::is_alphabetic) && !span.chars().any(char::is_lowercase) {
            continue;
        }
        if out.iter().any(|f| f.from < l.span.end && l.span.start < f.to) {
            continue;
        }
        let replacement = l.suggestions.first().map(|s| match s {
            Suggestion::ReplaceWith(c) => c.iter().collect(),
            Suggestion::InsertAfter(c) => format!("{span}{}", c.iter().collect::<String>()),
            Suggestion::Remove => String::new(),
        });
        out.push(Finding {
            from: l.span.start,
            to: l.span.end,
            message: l.message,
            replacement,
        });
    }
    out.sort_by_key(|f| f.from);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, Option<String>)> {
        let chars: Vec<char> = text.chars().collect();
        check(text, English::British)
            .into_iter()
            .map(|f| (chars[f.from..f.to].iter().collect(), f.replacement))
            .collect()
    }

    #[test]
    fn house_rules_and_harper() {
        assert_eq!(
            found("A MINISTER has been stood waiting. He humbled him self next the grave."),
            vec![
                ("stood".into(), Some("standing".into())),
                ("him self".into(), Some("himself".into())),
                ("next the".into(), Some("next to the".into())),
            ]
        );
        let f = found("Their going to the market, and its a long way.");
        assert!(f.iter().any(|(s, _)| s == "its"), "{f:?}");
        // Names in capitals and plain screenplay action aren't flagged.
        assert_eq!(
            found("Grey water. NELL (30s, oilskin, sleepless) walks the length of it."),
            vec![]
        );
        assert_eq!(
            found("Four men carry a coffin, the front right of whom is JOHN DEE."),
            vec![]
        );
    }
}
