//! Keyword ranking for code, used by Uncaged's local codebase index.
//!
//! Code is split into terms the way people search for it: whole identifiers
//! (`codebaseindexmanager`) plus their parts (`codebase`, `index`, `manager`),
//! lowercased and lightly stemmed so `tabs`, `dragging` and `dragged` meet
//! `tab` and `drag`. Chunks are ranked with BM25 over their content, with extra
//! weight when a query term appears in the file's path or in the first lines of
//! the chunk (where a function or type is usually named).

use std::collections::HashMap;
use std::path::Path;

/// BM25 term-frequency saturation.
const K1: f32 = 1.2;
/// BM25 length normalisation.
const B: f32 = 0.75;
/// Weight of a query term found in the file path, as a multiple of its IDF.
const PATH_WEIGHT: f32 = 1.0;
/// Weight of a query term found in a chunk's first lines, as a multiple of its IDF.
const HEAD_WEIGHT: f32 = 0.6;
/// Lines at the top of a chunk that count as its "head".
const HEAD_LINES: usize = 2;
/// Trailing path components that contribute path terms (file name plus parents).
const PATH_COMPONENTS: usize = 4;
/// Longest token kept; longer runs are usually hashes, base64 or minified code.
const MAX_TOKEN_LEN: usize = 48;

/// Words that carry no signal in code search, on either side.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on",
    "or", "that", "the", "this", "to", "with",
];

/// Extra words that only show up in how people phrase questions.
const QUERY_STOPWORDS: &[&str] = &[
    "all", "any", "can", "code", "codebase", "do", "doe", "does", "find", "how", "i", "me", "my",
    "please", "show", "some", "there", "we", "what", "when", "where", "which", "who", "why",
];

/// Interns terms so documents can store small integer ids instead of strings.
#[derive(Debug, Default)]
pub struct Vocab {
    ids: HashMap<Box<str>, u32>,
}

impl Vocab {
    pub fn id(&mut self, term: &str) -> u32 {
        if let Some(id) = self.ids.get(term) {
            return *id;
        }
        let id = self.ids.len() as u32;
        self.ids.insert(term.into(), id);
        id
    }

    pub fn get(&self, term: &str) -> Option<u32> {
        self.ids.get(term).copied()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// The searchable form of one chunk of code.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// (term id, count), sorted by term id.
    terms: Vec<(u32, u16)>,
    /// Total number of terms in the chunk.
    len: u32,
    /// Term ids found in the chunk's first lines, sorted and deduplicated.
    head: Vec<u32>,
    /// Term ids found in the file's path, sorted and deduplicated.
    path: Vec<u32>,
}

impl Document {
    pub fn new(content: &str, path: Option<&Path>, vocab: &mut Vocab) -> Self {
        let mut tokens = Vec::new();
        tokenize(content, &mut tokens);
        let len = tokens.len() as u32;

        let mut counts: HashMap<u32, u16> = HashMap::new();
        for token in &tokens {
            let count = counts.entry(vocab.id(token)).or_default();
            *count = count.saturating_add(1);
        }
        let mut terms: Vec<(u32, u16)> = counts.into_iter().collect();
        terms.sort_unstable_by_key(|(id, _)| *id);

        let head_text = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .take(HEAD_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        let head = term_set(&head_text, vocab);

        let path = path
            .map(|path| {
                let components: Vec<_> = path.components().collect();
                let start = components.len().saturating_sub(PATH_COMPONENTS);
                let mut text = String::new();
                for (index, component) in components[start..].iter().enumerate() {
                    let part = component.as_os_str().to_string_lossy();
                    // Drop the extension of the file name: `rs` or `ts` match
                    // nearly every file and only add noise.
                    let part = if index + start == components.len() - 1 {
                        Path::new(&*part)
                            .file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                            .unwrap_or_else(|| part.into_owned())
                    } else {
                        part.into_owned()
                    };
                    text.push_str(&part);
                    text.push(' ');
                }
                term_set(&text, vocab)
            })
            .unwrap_or_default();

        Self {
            terms,
            len,
            head,
            path,
        }
    }

    fn count(&self, term: u32) -> u16 {
        self.terms
            .binary_search_by_key(&term, |(id, _)| *id)
            .map(|index| self.terms[index].1)
            .unwrap_or(0)
    }

    fn in_head(&self, term: u32) -> bool {
        self.head.binary_search(&term).is_ok()
    }

    fn in_path(&self, term: u32) -> bool {
        self.path.binary_search(&term).is_ok()
    }

    fn mentions(&self, term: u32) -> bool {
        self.count(term) > 0 || self.in_path(term)
    }
}

fn term_set(text: &str, vocab: &mut Vocab) -> Vec<u32> {
    let mut tokens = Vec::new();
    tokenize(text, &mut tokens);
    let mut ids: Vec<u32> = tokens.iter().map(|token| vocab.id(token)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// A query reduced to the term ids the index knows about.
#[derive(Debug, Clone)]
pub struct Query {
    /// Known term ids, deduplicated.
    terms: Vec<u32>,
    /// Number of distinct meaningful words in the query, known or not.
    word_count: usize,
}

impl Query {
    /// Returns `None` when no word in the query appears anywhere in the index.
    pub fn new(query: &str, vocab: &Vocab) -> Option<Self> {
        let mut tokens = Vec::new();
        tokenize(query, &mut tokens);
        tokens.retain(|token| !QUERY_STOPWORDS.contains(&token.as_str()));
        tokens.sort_unstable();
        tokens.dedup();

        let word_count = tokens.len();
        let mut terms: Vec<u32> = tokens.iter().filter_map(|token| vocab.get(token)).collect();
        terms.sort_unstable();
        terms.dedup();
        if terms.is_empty() {
            return None;
        }
        Some(Self { terms, word_count })
    }
}

/// Scores `docs` against `query` and returns the best `limit` keys, best first.
/// Documents that match no query term are left out.
pub fn rank<K: Clone>(query: &Query, docs: &[(K, &Document)], limit: usize) -> Vec<(K, f32)> {
    if docs.is_empty() || limit == 0 {
        return Vec::new();
    }

    let doc_count = docs.len() as f32;
    let avg_len = (docs.iter().map(|(_, doc)| doc.len as f64).sum::<f64>() / docs.len() as f64)
        .max(1.0) as f32;
    let idf: Vec<f32> = query
        .terms
        .iter()
        .map(|term| {
            let df = docs.iter().filter(|(_, doc)| doc.mentions(*term)).count() as f32;
            (1.0 + (doc_count - df + 0.5) / (df + 0.5)).ln()
        })
        .collect();

    let mut scored: Vec<(K, f32)> = docs
        .iter()
        .filter_map(|(key, doc)| {
            let mut score = 0.0;
            let mut matched = 0usize;
            for (term, idf) in query.terms.iter().zip(&idf) {
                let tf = doc.count(*term) as f32;
                let in_path = doc.in_path(*term);
                if tf == 0.0 && !in_path {
                    continue;
                }
                matched += 1;
                if tf > 0.0 {
                    let norm = K1 * (1.0 - B + B * doc.len as f32 / avg_len);
                    score += idf * tf * (K1 + 1.0) / (tf + norm);
                    if doc.in_head(*term) {
                        score += HEAD_WEIGHT * idf;
                    }
                }
                if in_path {
                    score += PATH_WEIGHT * idf;
                }
            }
            if matched == 0 {
                return None;
            }
            // Prefer chunks that cover more of the question over chunks that
            // repeat one word many times.
            let coverage = matched as f32 / query.word_count.max(1) as f32;
            Some((key.clone(), score * (0.4 + 0.6 * coverage)))
        })
        .collect();

    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(limit);
    scored
}

/// Splits text into search terms: each identifier whole, plus its camelCase,
/// snake_case and letter/digit parts, lowercased and stemmed.
pub fn tokenize(text: &str, out: &mut Vec<String>) {
    for run in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        let run = run.trim_matches('_');
        if run.is_empty() || run.len() > MAX_TOKEN_LEN {
            continue;
        }

        let parts = split_identifier(run);
        let whole = run.replace('_', "").to_lowercase();
        if parts.len() > 1 {
            push_term(&whole, out);
        }
        for part in parts {
            push_term(&part.to_lowercase(), out);
        }
    }
}

fn push_term(term: &str, out: &mut Vec<String>) {
    if term.chars().count() < 2 || term.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    let stemmed = stem(term);
    if STOPWORDS.contains(&stemmed.as_str()) {
        return;
    }
    out.push(stemmed);
}

/// Splits `HTTPServerError2_retry` into `HTTP`, `Server`, `Error`, `2`, `retry`.
fn split_identifier(run: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = run.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for i in 1..chars.len() {
        let (index, current) = chars[i];
        let previous = chars[i - 1].1;
        let next = chars.get(i + 1).map(|(_, c)| *c);
        let boundary = current == '_'
            || previous == '_'
            || (previous.is_lowercase() && current.is_uppercase())
            || (previous.is_alphabetic() && current.is_numeric())
            || (previous.is_numeric() && current.is_alphabetic())
            // The last capital of an acronym starts the next word: HTTPServer.
            || (previous.is_uppercase()
                && current.is_uppercase()
                && next.is_some_and(|c| c.is_lowercase()));
        if boundary {
            let part = &run[start..index];
            if !part.is_empty() && part != "_" {
                parts.push(part.trim_matches('_'));
            }
            start = index;
        }
    }
    let last = run[start..].trim_matches('_');
    if !last.is_empty() {
        parts.push(last);
    }
    parts.retain(|part| !part.is_empty());
    parts
}

/// A deliberately small stemmer: plurals and -ing/-ed, enough for code words.
fn stem(word: &str) -> String {
    if !word.chars().all(|c| c.is_ascii_lowercase()) {
        return word.to_string();
    }
    let len = word.len();
    let undouble = |stem: &str| -> String {
        let bytes = stem.as_bytes();
        let n = bytes.len();
        if n >= 3
            && bytes[n - 1] == bytes[n - 2]
            && !matches!(
                bytes[n - 1],
                b'a' | b'e' | b'i' | b'o' | b'u' | b'l' | b's' | b'z'
            )
        {
            stem[..n - 1].to_string()
        } else {
            stem.to_string()
        }
    };

    let has_vowel = |stem: &str| {
        stem.chars()
            .any(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
    };

    // `string` and `spring` keep their ending: what's left has no vowel.
    if len > 5 && word.ends_with("ing") && has_vowel(&word[..len - 3]) {
        return undouble(&word[..len - 3]);
    }
    if len > 4 && word.ends_with("ies") {
        return format!("{}y", &word[..len - 3]);
    }
    if len > 4 && word.ends_with("ed") && !word.ends_with("eed") && has_vowel(&word[..len - 2]) {
        return undouble(&word[..len - 2]);
    }
    if len > 4
        && (word.ends_with("xes")
            || word.ends_with("ches")
            || word.ends_with("shes")
            || word.ends_with("sses"))
    {
        return word[..len - 2].to_string();
    }
    if len > 3
        && word.ends_with('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("is")
    {
        return word[..len - 1].to_string();
    }
    word.to_string()
}

#[cfg(test)]
#[path = "lexical_tests.rs"]
mod tests;
