use std::path::PathBuf;

use string_offset::ByteOffset;
use uncaged_ripgrep::search::Submatch;
use uncaged_util::local_or_remote_path::LocalOrRemotePath;

use super::GlobalSearch;
use crate::workspace::view::global_search::GlobalSearchMatch;

fn search_match(line_text: &str, submatches: Vec<(usize, usize)>) -> GlobalSearchMatch {
    GlobalSearchMatch {
        location: LocalOrRemotePath::Local(PathBuf::from("/repo/a.rs")),
        line_number: 1,
        column_num: None,
        line_text: line_text.to_string(),
        submatches: submatches
            .into_iter()
            .map(|(byte_start, byte_end)| Submatch {
                byte_start: ByteOffset::from(byte_start),
                byte_end: ByteOffset::from(byte_end),
            })
            .collect(),
    }
}

#[test]
fn matches_expand_one_row_per_submatch() {
    let results = GlobalSearch::expand_submatches(search_match("foo foo", vec![(0, 3), (4, 7)]));

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|m| m.submatches.len() == 1));
}

#[test]
fn match_leading_whitespace_is_trimmed_per_submatch() {
    let results = GlobalSearch::expand_submatches(search_match("    foo", vec![(4, 7)]));

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].line_text, "foo");
    assert_eq!(results[0].column_num, Some(5));
    assert_eq!(results[0].submatches[0].byte_start.as_usize(), 0);
    assert_eq!(results[0].submatches[0].byte_end.as_usize(), 3);
}

#[test]
fn match_column_counts_characters_not_bytes() {
    let results = GlobalSearch::expand_submatches(search_match("€foo", vec![(3, 6)]));

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].column_num, Some(2));
}
