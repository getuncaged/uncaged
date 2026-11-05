use std::path::Path;

use super::*;

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    tokenize(text, &mut out);
    out
}

#[test]
fn splits_identifiers_and_keeps_the_whole() {
    let out = tokens("CodebaseIndexManager");
    assert!(out.contains(&"codebaseindexmanager".to_string()));
    assert!(out.contains(&"codebase".to_string()));
    assert!(out.contains(&"index".to_string()));
    assert!(out.contains(&"manager".to_string()));

    let out = tokens("begin_tab_reorder");
    assert!(out.contains(&"begintabreorder".to_string()));
    assert!(out.contains(&"begin".to_string()));
    assert!(out.contains(&"tab".to_string()));
    assert!(out.contains(&"reorder".to_string()));
}

#[test]
fn splits_acronyms_and_digits() {
    let out = tokens("HTTPServerError2");
    assert!(out.contains(&"http".to_string()));
    assert!(out.contains(&"server".to_string()));
    assert!(out.contains(&"error".to_string()));
    // Lone digits are dropped.
    assert!(!out.contains(&"2".to_string()));
}

#[test]
fn stems_plurals_and_verb_endings() {
    assert_eq!(stem("tabs"), "tab");
    assert_eq!(stem("dragging"), "drag");
    assert_eq!(stem("dragged"), "drag");
    assert_eq!(stem("indexes"), "index");
    assert_eq!(stem("entries"), "entry");
    assert_eq!(stem("class"), "class");
    assert_eq!(stem("status"), "status");
    assert_eq!(stem("string"), "string");
    assert_eq!(stem("installed"), "install");
}

#[test]
fn drops_stopwords_and_short_tokens() {
    let out = tokens("the a x of value");
    assert_eq!(out, vec!["value".to_string()]);
}

#[test]
fn query_with_no_known_words_is_none() {
    let mut vocab = Vocab::default();
    Document::new("fn render_tab_bar() {}", None, &mut vocab);
    assert!(Query::new("where is the kubernetes operator", &vocab).is_none());
    assert!(Query::new("how do we render tabs", &vocab).is_some());
}

#[test]
fn ranks_the_chunk_that_names_the_concept_first() {
    let mut vocab = Vocab::default();
    let drag = Document::new(
        "fn begin_tab_drag(&mut self, tab: TabId) {\n    self.dragging = Some(tab);\n}",
        Some(Path::new("/repo/app/src/workspace/tab_drag.rs")),
        &mut vocab,
    );
    let theme = Document::new(
        "fn apply_theme(&mut self, theme: Theme) {\n    self.theme = theme;\n}",
        Some(Path::new("/repo/app/src/appearance/theme.rs")),
        &mut vocab,
    );
    let tabs_mention = Document::new(
        "// Tabs are rendered elsewhere.\nfn render_toolbar(&self) {}",
        Some(Path::new("/repo/app/src/toolbar.rs")),
        &mut vocab,
    );
    let docs = vec![
        ("drag", &drag),
        ("theme", &theme),
        ("toolbar", &tabs_mention),
    ];

    let query = Query::new("where do we handle dragging tabs", &vocab).unwrap();
    let ranked = rank(&query, &docs, 10);
    assert_eq!(ranked[0].0, "drag");
    // The theme chunk shares no words with the question.
    assert!(ranked.iter().all(|(key, _)| *key != "theme"));
}

#[test]
fn path_match_counts_without_content_match() {
    let mut vocab = Vocab::default();
    let settings = Document::new(
        "pub fn render(&self) {}",
        Some(Path::new("/repo/app/src/settings_view/keybindings.rs")),
        &mut vocab,
    );
    let other = Document::new(
        "pub fn render(&self) {}",
        Some(Path::new("/repo/app/src/terminal/view.rs")),
        &mut vocab,
    );
    let docs = vec![("settings", &settings), ("other", &other)];
    let query = Query::new("keybindings render", &vocab).unwrap();
    let ranked = rank(&query, &docs, 10);
    assert_eq!(ranked[0].0, "settings");
}

#[test]
fn rank_respects_limit_and_empty_input() {
    let mut vocab = Vocab::default();
    let a = Document::new("alpha beta", None, &mut vocab);
    let b = Document::new("alpha gamma", None, &mut vocab);
    let query = Query::new("alpha", &vocab).unwrap();
    assert_eq!(rank(&query, &[(1, &a), (2, &b)], 1).len(), 1);
    assert!(rank::<u8>(&query, &[], 5).is_empty());
}
