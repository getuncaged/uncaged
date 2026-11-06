use ::ai::index::locations::FileFragmentLocation;

use super::*;

fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    dir
}

#[test]
fn shows_matches_with_paths_and_numbered_lines() {
    let repo = repo_with(&[(
        "src/tabs.rs",
        "line one\nfn drag_tab() {}\nline three\nline four\n",
    )]);
    let answer = render_matches(
        "drag tab",
        repo.path(),
        vec![CodeContextLocation::Fragment(FileFragmentLocation {
            path: repo.path().join("src/tabs.rs"),
            line_ranges: std::iter::once(1..3).collect(),
        })],
    );
    assert!(answer.contains("src/tabs.rs:2-3"), "{answer}");
    assert!(answer.contains("     2  fn drag_tab() {}"), "{answer}");
    assert!(answer.contains("     3  line three"), "{answer}");
    assert!(!answer.contains("line one"), "{answer}");
    assert!(!answer.contains("line four"), "{answer}");
}

#[test]
fn long_matches_are_cut() {
    let content: String = (1..=200).map(|n| format!("line {n}\n")).collect();
    let repo = repo_with(&[("big.rs", &content)]);
    let answer = render_matches(
        "line",
        repo.path(),
        vec![CodeContextLocation::Fragment(FileFragmentLocation {
            path: repo.path().join("big.rs"),
            line_ranges: std::iter::once(0..200).collect(),
        })],
    );
    assert!(answer.contains("big.rs:1-200"), "{answer}");
    assert!(answer.contains("line 80\n"), "{answer}");
    assert!(!answer.contains("line 81\n"), "{answer}");
    assert!(answer.contains("120 more lines"), "{answer}");
}

#[test]
fn whole_files_and_changed_files() {
    let repo = repo_with(&[("a.rs", "fn a() {}\n"), ("b.rs", "short\n")]);
    let answer = render_matches(
        "a",
        repo.path(),
        vec![
            CodeContextLocation::WholeFile(repo.path().join("a.rs")),
            CodeContextLocation::Fragment(FileFragmentLocation {
                path: repo.path().join("b.rs"),
                line_ranges: std::iter::once(10..20).collect(),
            }),
        ],
    );
    assert!(answer.contains("a.rs:1-1"), "{answer}");
    assert!(answer.contains("     1  fn a() {}"), "{answer}");
    assert!(
        answer.contains("the file changed since it was indexed"),
        "{answer}"
    );
}

#[test]
fn no_matches_says_so() {
    let answer = render_matches("kubernetes", Path::new("/repo"), Vec::new());
    assert!(
        answer.starts_with("No matches for \"kubernetes\""),
        "{answer}"
    );
}
