//! Shared filter for user-visible short text: display names, handles, captions, board titles
//! (SR-28, threat-model A-21; prd-mvp AC-3.2, AC-12).
//!
//! Two jobs, kept separate:
//!
//! 1. **Sanitize** ([`TextFilter::sanitize`]): produce a safe, single-line display string.
//!    NFC normalization; control, bidi-control and invisible characters removed; whitespace
//!    runs collapsed to one space and trimmed; combining marks capped per grapheme (zalgo);
//!    over-long grapheme clusters dropped. Zero-width joiners survive only where they join a
//!    grapheme (emoji ZWJ sequences, Indic half forms), non-joiners only between non-ASCII
//!    letters. Sanitizing is idempotent and never adds graphemes.
//! 2. **Block** ([`TextFilter::blocked`]): match a folded key of the text against the term
//!    lists from `config/text_filter.yaml` (profanity, sexual terms, slurs). The key undoes
//!    common evasions: compatibility forms (fullwidth, math letters), accents, case, Cyrillic
//!    and Greek look-alikes, digit/symbol substitutions, spelled-out letters (`f.u.c.k`) and
//!    stretched letters (`fuuuck`). `words` terms match whole words only, so `Scunthorpe` and
//!    `Cassandra` pass; `contains` terms match inside words.
//!
//! [`TextFilter::check`] does both plus the length rule of a field, counted in grapheme
//! clusters (what a person sees as one character). Error values never echo the matched term.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{canonical_combining_class, is_combining_mark};
use unicode_segmentation::UnicodeSegmentation;
use utoipa::ToSchema;

const SUPPORTED_VERSION: u32 = 1;
const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';
/// Canonical combining class of viramas (Indic conjunct linkers), exempt from the mark cap.
const VIRAMA_CCC: u8 = 9;
/// Raw input above this many bytes per allowed grapheme is refused before any processing.
const MAX_INPUT_BYTES_PER_GRAPHEME: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum TextFilterConfigError {
    #[error("cannot read text filter config {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid text filter config YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("unsupported text filter config version {found} (supported: {SUPPORTED_VERSION})")]
    UnsupportedVersion { found: u32 },
    #[error("invalid text filter config: {0}")]
    Invalid(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: u32,
    max_marks_per_grapheme: usize,
    max_chars_per_grapheme: usize,
    lists: BTreeMap<String, HintList>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HintList {
    /// Blocked when the folded text has a whole word equal to the term.
    #[serde(default)]
    words: Vec<String>,
    /// Blocked when the term appears anywhere inside a folded word (or a spelled-out run of
    /// single letters such as `f.u.c.k`). Use for terms that rarely occur inside innocent words.
    #[serde(default)]
    contains: Vec<String>,
}

/// The filter's settings as served to clients in `GET /v1/config` (#213): the sanitizer caps and
/// the term lists exactly as written in `config/text_filter.yaml`. For instant hints in the app
/// only; the server's own check stays authoritative. Public by design (EM decision 2026-11-05).
#[derive(Debug, Clone, Default, Serialize, ToSchema, PartialEq, Eq)]
pub struct TextFilterHints {
    pub version: u32,
    /// Combining marks kept per grapheme cluster (viramas exempt).
    pub max_marks_per_grapheme: usize,
    /// Grapheme clusters with more code points than this are dropped.
    pub max_chars_per_grapheme: usize,
    /// List name -> terms.
    pub lists: BTreeMap<String, HintList>,
}

/// Length rule of one field, in grapheme clusters of the sanitized text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldRules {
    pub min_graphemes: usize,
    pub max_graphemes: usize,
}

/// Why a text was refused. Safe to show; never contains the matched term.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    #[error("must be at least {min} characters")]
    TooShort { min: usize },
    #[error("must be at most {max} characters")]
    TooLong { max: usize },
    /// Matched a term list. `list` is the config list name, for logs and metrics only.
    #[error("is not allowed")]
    Blocked { list: String },
}

#[derive(Debug, Clone)]
struct TermList {
    name: String,
    words: HashSet<String>,
    contains: Vec<String>,
}

/// Sanitizer and term matcher configured from `config/text_filter.yaml`.
#[derive(Debug, Clone)]
pub struct TextFilter {
    max_marks_per_grapheme: usize,
    max_chars_per_grapheme: usize,
    lists: Vec<TermList>,
    hints: TextFilterHints,
}

impl TextFilter {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, TextFilterConfigError> {
        let raw: RawConfig = serde_norway::from_str(yaml)?;
        if raw.version != SUPPORTED_VERSION {
            return Err(TextFilterConfigError::UnsupportedVersion { found: raw.version });
        }
        if raw.max_chars_per_grapheme == 0 {
            return Err(TextFilterConfigError::Invalid(
                "max_chars_per_grapheme must be at least 1".to_owned(),
            ));
        }
        let hints = TextFilterHints {
            version: raw.version,
            max_marks_per_grapheme: raw.max_marks_per_grapheme,
            max_chars_per_grapheme: raw.max_chars_per_grapheme,
            lists: raw.lists.clone(),
        };
        let mut lists = Vec::with_capacity(raw.lists.len());
        for (name, list) in raw.lists {
            let fold_term = |term: &String| {
                let folded = fold(term).concat();
                if folded.is_empty() {
                    Err(TextFilterConfigError::Invalid(format!(
                        "list {name:?} has a term that folds to nothing"
                    )))
                } else {
                    Ok(folded)
                }
            };
            lists.push(TermList {
                words: list.words.iter().map(fold_term).collect::<Result<_, _>>()?,
                contains: list
                    .contains
                    .iter()
                    .map(fold_term)
                    .collect::<Result<_, _>>()?,
                name,
            });
        }
        Ok(Self {
            max_marks_per_grapheme: raw.max_marks_per_grapheme,
            max_chars_per_grapheme: raw.max_chars_per_grapheme,
            lists,
            hints,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, TextFilterConfigError> {
        let yaml = std::fs::read_to_string(path).map_err(|source| TextFilterConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml_str(&yaml)
    }

    /// The client hint copy of this filter, served by `GET /v1/config`.
    pub fn client_hints(&self) -> &TextFilterHints {
        &self.hints
    }

    /// Sanitizes, then applies the length rule and the term lists. Returns the sanitized text,
    /// which is what must be stored and shown.
    pub fn check(&self, input: &str, rules: FieldRules) -> Result<String, TextError> {
        let byte_cap = rules
            .max_graphemes
            .saturating_mul(MAX_INPUT_BYTES_PER_GRAPHEME);
        if input.len() > byte_cap {
            return Err(TextError::TooLong {
                max: rules.max_graphemes,
            });
        }
        let clean = self.sanitize(input);
        let length = grapheme_len(&clean);
        if length < rules.min_graphemes {
            return Err(TextError::TooShort {
                min: rules.min_graphemes,
            });
        }
        if length > rules.max_graphemes {
            return Err(TextError::TooLong {
                max: rules.max_graphemes,
            });
        }
        if let Some(list) = self.blocked(&clean) {
            return Err(TextError::Blocked {
                list: list.to_owned(),
            });
        }
        Ok(clean)
    }

    /// Single-line display form of `input` (see the module docs). Idempotent.
    pub fn sanitize(&self, input: &str) -> String {
        // One pass is designed to be a fixed point; repeating until stable guards against
        // rare grapheme-boundary shifts (e.g. a dropped joiner merging two clusters).
        let mut current = self.sanitize_pass(input);
        for _ in 0..3 {
            let next = self.sanitize_pass(&current);
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn sanitize_pass(&self, input: &str) -> String {
        // 1. Canonical composition; drop invisible and control characters; any whitespace
        //    becomes a plain space.
        let visible: String = input
            .nfc()
            .filter_map(|c| {
                if c.is_whitespace() {
                    Some(' ')
                } else if is_stripped(c) {
                    None
                } else {
                    Some(c)
                }
            })
            .collect();

        // 2. Collapse whitespace runs and trim.
        let collapsed = visible.split(' ').filter(|w| !w.is_empty()).fold(
            String::with_capacity(visible.len()),
            |mut acc, word| {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(word);
                acc
            },
        );

        // 3. Per grapheme: cap combining marks, keep a ZWJ only where it joins the cluster to a
        //    following non-mark character, then drop clusters that are still over-long.
        let mut out = String::with_capacity(collapsed.len());
        for grapheme in collapsed.graphemes(true) {
            if grapheme.chars().all(is_hangul_filler) {
                continue;
            }
            let chars: Vec<char> = grapheme.chars().collect();
            let mut cluster = String::with_capacity(grapheme.len());
            let mut marks = 0usize;
            for (i, &c) in chars.iter().enumerate() {
                if c == ZWJ {
                    let joins = chars
                        .get(i + 1)
                        .is_some_and(|&next| next != ZWJ && !is_combining_mark(next));
                    if joins {
                        cluster.push(c);
                    }
                } else if is_combining_mark(c) && canonical_combining_class(c) != VIRAMA_CCC {
                    // Viramas link Indic conjuncts into one cluster; dropping one would split it.
                    marks += 1;
                    if marks <= self.max_marks_per_grapheme {
                        cluster.push(c);
                    }
                } else {
                    cluster.push(c);
                }
            }
            if cluster.chars().count() <= self.max_chars_per_grapheme {
                out.push_str(&cluster);
            }
        }

        // 4. A non-joiner only matters between letters of scripts that use it (Persian,
        //    Indic); anywhere else it is an invisible evasion character.
        let chars: Vec<char> = out.chars().collect();
        let kept: String = chars
            .iter()
            .enumerate()
            .filter(|&(i, &c)| {
                c != ZWNJ || {
                    let letter = |j: Option<usize>| {
                        j.and_then(|j| chars.get(j))
                            .is_some_and(|&n| n.is_alphabetic() && !n.is_ascii())
                    };
                    letter(i.checked_sub(1)) && letter(i.checked_add(1))
                }
            })
            .map(|(_, &c)| c)
            .collect();

        kept.nfc().collect()
    }

    /// The name of the first term list `text` matches, if any. Run on sanitized text.
    pub fn blocked(&self, text: &str) -> Option<&str> {
        // Each word as written and stretched-out (`fuuuck`), plus spelled-out runs of single
        // letters joined back together (`f.u.c.k`, `s m u t`).
        let candidates: Vec<(String, String)> = candidate_words(fold(text))
            .into_iter()
            .map(|word| {
                let stretched = collapse_runs(&word);
                (word, stretched)
            })
            .collect();
        // A stretched form is only compared with terms that have no doubled letters, so
        // `nigeria` never matches a term spelled with a double letter.
        let plain = |term: &String| collapse_runs(term) == *term;
        self.lists
            .iter()
            .find(|list| {
                candidates.iter().any(|(word, stretched)| {
                    list.words.contains(word)
                        || (list.words.contains(stretched) && plain(stretched))
                        || list.contains.iter().any(|term| {
                            word.contains(term.as_str())
                                || (plain(term) && stretched.contains(term.as_str()))
                        })
                })
            })
            .map(|list| list.name.as_str())
    }
}

/// Number of extended grapheme clusters (user-perceived characters).
pub fn grapheme_len(text: &str) -> usize {
    text.graphemes(true).count()
}

/// Characters removed from display text: controls, bidi controls, invisible formatting,
/// tag characters, variation-selector supplement, private use and noncharacters. ZWJ and
/// ZWNJ are kept here and decided by context later.
fn is_stripped(c: char) -> bool {
    let cp = u32::from(c);
    c.is_control()
        || matches!(
            cp,
            0x00AD                      // soft hyphen
            | 0x034F                    // combining grapheme joiner
            | 0x061C                    // Arabic letter mark
            | 0x17B4 | 0x17B5           // Khmer inherent vowels (invisible)
            | 0x180B..=0x180F           // Mongolian variation selectors, vowel separator
            | 0x200B                    // zero width space
            | 0x200E | 0x200F           // LRM, RLM
            | 0x202A..=0x202E           // bidi embeddings and overrides
            | 0x2060..=0x206F           // word joiner, invisible operators, bidi isolates, deprecated
            | 0x2800                    // braille blank (renders empty)
            | 0x3164 | 0xFFA0           // Hangul fillers
            | 0xFE00..=0xFE0E           // variation selectors 1-15 (VS16 U+FE0F stays for emoji)
            | 0xFEFF                    // zero width no-break space / BOM
            | 0xFFF0..=0xFFFB           // specials, interlinear annotation
            | 0x1D173..=0x1D17A         // musical formatting controls
            | 0xE0000..=0xE007F         // tag characters
            | 0xE0100..=0xE01EF         // variation selectors supplement
            | 0xE000..=0xF8FF           // private use
            | 0xF0000..=0x10FFFF        // supplementary private use planes
        )
        || (cp & 0xFFFE) == 0xFFFE // noncharacters U+xxFFFE / U+xxFFFF
        || (0xFDD0..=0xFDEF).contains(&cp)
}

/// Hangul choseong/jungseong fillers render as nothing on their own. Inside a syllable they
/// belong to the cluster, so they are dropped only when they form a cluster alone.
fn is_hangul_filler(c: char) -> bool {
    matches!(c, '\u{115F}' | '\u{1160}')
}

/// Folds text into lowercase ASCII-ish words for term matching: compatibility decomposition,
/// accents removed, lowercase, look-alike and digit/symbol substitutions, split on anything
/// that is not a letter or digit.
fn fold(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.nfkd().filter(|c| !is_combining_mark(*c)) {
        for lower in c.to_lowercase() {
            match substitute(lower) {
                Some(mapped) => word.push_str(mapped),
                None if lower.is_alphanumeric() => word.push(lower),
                None => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Look-alikes and leetspeak mapped to the Latin letter they imitate.
fn substitute(c: char) -> Option<&'static str> {
    Some(match c {
        '0' | 'ο' | 'о' | 'ø' => "o",
        '1' | '!' | '|' | 'ı' | 'і' | 'ι' | 'ї' => "i",
        '3' | 'е' | 'ε' | 'є' | 'ё' => "e",
        '4' | '@' | 'а' | 'α' => "a",
        '5' | '$' | 'ѕ' => "s",
        '7' | '+' | 'т' | 'τ' => "t",
        '8' | 'в' | 'β' => "b",
        '9' => "g",
        'с' | 'ϲ' | '¢' => "c",
        'р' | 'ρ' => "p",
        'у' | 'γ' => "y",
        'х' | 'χ' => "x",
        'к' | 'κ' => "k",
        'м' => "m",
        'н' | 'η' => "h",
        'ј' => "j",
        'ԁ' => "d",
        'ν' => "v",
        'υ' => "u",
        'ш' | 'ω' => "w",
        'ł' => "l",
        '\u{0111}' | 'ð' => "d",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => return None,
    })
}

/// The folded words plus every run of two or more consecutive one-letter words joined.
fn candidate_words(words: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(words.len() + 1);
    let mut run = String::new();
    let mut run_len = 0usize;
    for word in words {
        if word.chars().count() == 1 {
            run.push_str(&word);
            run_len += 1;
        } else {
            if run_len >= 2 {
                out.push(std::mem::take(&mut run));
            }
            run.clear();
            run_len = 0;
        }
        out.push(word);
    }
    if run_len >= 2 {
        out.push(run);
    }
    out
}

/// `fuuuuck` -> `fuck`: runs of one character collapsed to a single one.
fn collapse_runs(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut last = None;
    for c in word.chars() {
        if last != Some(c) {
            out.push(c);
        }
        last = Some(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neutral stand-in terms: the real lists live in config/text_filter.yaml.
    const TEST_YAML: &str = r"
version: 1
max_marks_per_grapheme: 2
max_chars_per_grapheme: 12
lists:
  profanity:
    words: [darn, heck]
    contains: [frack]
  sexual:
    contains: [smut]
";

    fn filter() -> TextFilter {
        TextFilter::from_yaml_str(TEST_YAML).unwrap()
    }

    const NAME: FieldRules = FieldRules {
        min_graphemes: 2,
        max_graphemes: 30,
    };

    #[test]
    fn client_hints_are_the_terms_as_written() {
        let hints = filter().client_hints().clone();
        assert_eq!(hints.version, 1);
        assert_eq!(hints.max_marks_per_grapheme, 2);
        assert_eq!(hints.max_chars_per_grapheme, 12);
        assert_eq!(hints.lists.len(), 2);
        let profanity = &hints.lists["profanity"];
        assert_eq!(profanity.words, ["darn", "heck"]);
        assert_eq!(profanity.contains, ["frack"]);
        assert!(hints.lists["sexual"].words.is_empty());
    }

    #[test]
    fn nfc_normalizes_decomposed_accents() {
        let f = filter();
        // "Vie" + combining dot below + combining circumflex + "t" -> precomposed.
        assert_eq!(f.sanitize("Vie\u{0323}\u{0302}t"), "Vi\u{1EC7}t");
        assert_eq!(f.sanitize("Mai"), "Mai");
    }

    #[test]
    fn bidi_controls_and_overrides_are_removed() {
        let f = filter();
        // Right-to-left override makes "gpj.exe" display as "exe.jpg".
        assert_eq!(f.sanitize("abc\u{202E}gpj.exe"), "abcgpj.exe");
        for c in [
            '\u{200E}', '\u{200F}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            assert_eq!(
                f.sanitize(&format!("a{c}b")),
                "ab",
                "U+{:04X}",
                u32::from(c)
            );
        }
    }

    #[test]
    fn zero_width_and_invisible_characters_are_removed() {
        let f = filter();
        for c in [
            '\u{200B}',
            '\u{2060}',
            '\u{FEFF}',
            '\u{00AD}',
            '\u{034F}',
            '\u{3164}',
            '\u{2800}',
            '\u{E0041}',
            '\u{E000}',
        ] {
            assert_eq!(
                f.sanitize(&format!("m{c}ai")),
                "mai",
                "U+{:04X}",
                u32::from(c)
            );
        }
        // A name made only of invisible characters is empty, hence too short.
        assert_eq!(
            f.check("\u{3164}\u{3164}\u{200B}", NAME),
            Err(TextError::TooShort { min: 2 })
        );
    }

    #[test]
    fn controls_and_whitespace_become_single_spaces() {
        let f = filter();
        assert_eq!(
            f.sanitize("  Mai\t\n\r  the\u{3000}cat\u{00A0} "),
            "Mai the cat"
        );
        assert_eq!(f.sanitize("a\u{0007}b\u{0000}c\u{0085}d"), "abc d");
    }

    #[test]
    fn zalgo_marks_are_capped_per_grapheme() {
        let f = filter();
        let zalgo = format!("Z{}a{}", "\u{0336}\u{0301}\u{0310}".repeat(10), "\u{0352}");
        let clean = f.sanitize(&zalgo);
        assert_eq!(grapheme_len(&clean), 2);
        let marks = clean.chars().filter(|c| is_combining_mark(*c)).count();
        assert!(marks <= 3, "{clean:?}");
        assert!(f.check(&zalgo, NAME).is_ok());
    }

    #[test]
    fn emoji_sequences_and_joiners_survive_where_they_join() {
        let f = filter();
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(f.sanitize(family), family);
        assert_eq!(grapheme_len(family), 1);
        let rainbow_flag = "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}";
        assert_eq!(f.sanitize(rainbow_flag), rainbow_flag);
        let skin_tone = "\u{1F44B}\u{1F3FD}";
        assert_eq!(f.sanitize(skin_tone), skin_tone);
        // A joiner between letters joins nothing: removed.
        assert_eq!(f.sanitize("m\u{200D}ai"), "mai");
        // A non-joiner stays between Persian letters only.
        let persian = "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645}";
        assert_eq!(f.sanitize(persian), persian);
        assert_eq!(f.sanitize("m\u{200C}ai"), "mai");
    }

    #[test]
    fn overlong_grapheme_clusters_are_dropped() {
        let f = filter();
        let chain = std::iter::repeat_n("\u{1F600}", 20)
            .collect::<Vec<_>>()
            .join("\u{200D}");
        assert_eq!(grapheme_len(&chain), 1);
        assert_eq!(f.sanitize(&format!("ok{chain}")), "ok");
    }

    #[test]
    fn lengths_count_graphemes_not_bytes() {
        let f = filter();
        assert_eq!(f.check("Mi", NAME).as_deref(), Ok("Mi"));
        assert_eq!(f.check("M", NAME), Err(TextError::TooShort { min: 2 }));
        let thirty = "\u{1F431}".repeat(30);
        assert_eq!(f.check(&thirty, NAME).as_deref(), Ok(thirty.as_str()));
        assert_eq!(
            f.check(&"a".repeat(31), NAME),
            Err(TextError::TooLong { max: 30 })
        );
        // Oversized raw input is refused before processing.
        assert_eq!(
            f.check(&" ".repeat(10_000), NAME),
            Err(TextError::TooLong { max: 30 })
        );
    }

    #[test]
    fn whole_word_terms_match_words_only() {
        let f = filter();
        assert_eq!(f.blocked("oh darn it"), Some("profanity"));
        assert_eq!(f.blocked("mai.heck"), Some("profanity"));
        // Inside a longer word it is fine (no Scunthorpe problem).
        assert_eq!(f.blocked("darnell"), None);
        assert_eq!(f.blocked("checkers"), None);
    }

    #[test]
    fn contains_terms_match_across_separators() {
        let f = filter();
        assert_eq!(f.blocked("superfrackcat"), Some("profanity"));
        assert_eq!(f.blocked("f.r.a.c.k"), Some("profanity"));
        assert_eq!(f.blocked("s m u t"), Some("sexual"));
    }

    #[test]
    fn evasions_are_folded_before_matching() {
        let f = filter();
        for evasion in [
            "DARN",
            "D4RN",
            "d\u{0430}rn",        // Cyrillic a
            "\u{FF44}\u{FF41}rn", // fullwidth d, a
            "\u{1D41D}arn",       // mathematical bold d
            "da\u{0301}rn",       // accent
            "daaaarn",            // stretched
            "fr@ck",              // symbol
            "sm\u{200B}ut",       // zero-width space (after sanitize)
            "\u{0455}mut",        // Cyrillic dze
        ] {
            let clean = f.sanitize(evasion);
            assert!(f.blocked(&clean).is_some(), "{evasion:?} -> {clean:?}");
            assert!(matches!(
                f.check(evasion, NAME),
                Err(TextError::Blocked { .. })
            ));
        }
    }

    #[test]
    fn blocked_error_never_echoes_the_term() {
        let f = filter();
        let err = f.check("darn", NAME).unwrap_err();
        assert!(!err.to_string().contains("darn"));
        assert_eq!(err.to_string(), "is not allowed");
    }

    #[test]
    fn clean_names_pass() {
        let f = filter();
        for name in [
            "Mai",
            "Nguy\u{1EC5}n V\u{0103}n An",
            "José",
            "Zoë & Biscuit",
            "猫ちゃん",
            "Ölçek",
        ] {
            assert!(f.check(name, NAME).is_ok(), "{name}");
        }
    }

    #[test]
    fn config_rejects_bad_files() {
        assert!(matches!(
            TextFilter::from_yaml_str(&TEST_YAML.replace("version: 1", "version: 2")),
            Err(TextFilterConfigError::UnsupportedVersion { found: 2 })
        ));
        let empty_term = "version: 1\nmax_marks_per_grapheme: 2\nmax_chars_per_grapheme: 8\nlists:\n  x:\n    words: ['...']\n";
        assert!(matches!(
            TextFilter::from_yaml_str(empty_term),
            Err(TextFilterConfigError::Invalid(_))
        ));
        let unknown = format!("{TEST_YAML}extra: 1\n");
        assert!(TextFilter::from_yaml_str(&unknown).is_err());
    }

    #[test]
    fn repository_config_loads_and_blocks() {
        let yaml = include_str!("../../../config/text_filter.yaml");
        let f = TextFilter::from_yaml_str(yaml).unwrap();
        assert!(f.check("Mai the cat", NAME).is_ok());
        assert!(f.check("Scunthorpe", NAME).is_ok());
        assert!(f.check("Cassandra", NAME).is_ok());
        // Build the probe at runtime so the source holds no literal term.
        let probe: String = ['f', 'u', 'c', 'k'].iter().collect();
        assert!(matches!(
            f.check(&format!("mai {probe}"), NAME),
            Err(TextError::Blocked { .. })
        ));
    }

    /// Deterministic xorshift generator: property tests without a dependency or flakiness.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            let len = u64::try_from(items.len()).unwrap();
            &items[usize::try_from(self.next() % len).unwrap()]
        }
    }

    /// Characters that stress normalization, segmentation and stripping.
    const ALPHABET: &[char] = &[
        'a',
        'Z',
        'e',
        '0',
        '.',
        '_',
        ' ',
        '\t',
        '\n',
        '\u{00A0}',
        '\u{3000}',
        '\u{0301}',
        '\u{0323}',
        '\u{0302}',
        '\u{0336}',
        '\u{FE0F}',
        '\u{200D}',
        '\u{200C}',
        '\u{200B}',
        '\u{202E}',
        '\u{2066}',
        '\u{FEFF}',
        '\u{00AD}',
        '\u{034F}',
        '\u{1F468}',
        '\u{1F469}',
        '\u{1F3FD}',
        '\u{1F1FA}',
        '\u{1F1F8}',
        '\u{00A9}',
        '\u{2194}',
        '\u{0915}',
        '\u{094D}',
        '\u{0937}',
        '\u{0645}',
        '\u{06CC}',
        '\u{1100}',
        '\u{1161}',
        '\u{11A8}',
        '\u{115F}',
        '\u{1160}',
        '\u{3164}',
        '\u{0430}',
        '\u{FF41}',
        '\u{E0041}',
        '\u{0007}',
        'é',
        '\u{1EC7}',
        '\u{1F600}',
    ];

    fn random_text(rng: &mut Rng) -> String {
        let len = usize::try_from(rng.next() % 24).unwrap();
        (0..len).map(|_| *rng.pick(ALPHABET)).collect()
    }

    #[test]
    fn property_sanitize_is_idempotent_and_never_adds_graphemes() {
        let f = filter();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..5_000 {
            let input = random_text(&mut rng);
            let once = f.sanitize(&input);
            assert_eq!(f.sanitize(&once), once, "not idempotent for {input:?}");
            assert!(
                grapheme_len(&once) <= grapheme_len(&input),
                "grew: {input:?} -> {once:?}"
            );
            assert!(!once.starts_with(' ') && !once.ends_with(' '), "{once:?}");
            assert!(!once.contains("  "), "{once:?}");
            assert!(
                !once.chars().any(|c| c.is_control() || is_stripped(c)),
                "{once:?}"
            );
        }
    }

    #[test]
    fn property_check_is_stable_on_its_own_output() {
        let f = filter();
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let rules = FieldRules {
            min_graphemes: 1,
            max_graphemes: 30,
        };
        for _ in 0..2_000 {
            let input = random_text(&mut rng);
            if let Ok(clean) = f.check(&input, rules) {
                assert_eq!(f.check(&clean, rules).as_deref(), Ok(clean.as_str()));
            }
        }
    }
}
