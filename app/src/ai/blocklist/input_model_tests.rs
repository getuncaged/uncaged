use futures::executor::block_on;

use super::*;

#[test]
fn real_quick_ratio_matches_difflib_formula() {
    // Expected values are what Warp's difflib fork returned from
    // `SequenceMatcher::real_quick_ratio` for the same pairs.
    assert_eq!(real_quick_ratio("", ""), 1.0);
    assert_eq!(real_quick_ratio("abc", ""), 0.0);
    assert_eq!(real_quick_ratio("", "abc"), 0.0);
    assert_eq!(real_quick_ratio("git status", "git stauts"), 1.0);
    assert_eq!(real_quick_ratio("abcd", "wxyz"), 1.0);
    assert_eq!(real_quick_ratio("ls", "ls -la"), 0.5);
    assert_eq!(real_quick_ratio("cargo build", "cargo test"), 20.0 / 21.0);
    // Lengths are in bytes, as the matcher compares `&str`s byte by byte.
    assert_eq!(real_quick_ratio("é", "e"), 2.0 / 3.0);
}

#[test]
fn real_quick_ratio_is_an_upper_bound_on_ratio() {
    let pairs = [
        ("git status", "git stauts"),
        ("abcd", "wxyz"),
        ("ls", "ls -la"),
        ("cargo build", "cargo test"),
        ("é", "e"),
        ("", "abc"),
    ];
    for (a, b) in pairs {
        let mut matcher = difflib::sequencematcher::SequenceMatcher::new(a, b);
        assert!(
            real_quick_ratio(a, b) >= matcher.ratio(),
            "real_quick_ratio({a:?}, {b:?}) is below ratio()"
        );
    }
}

#[test]
fn has_any_close_matches_results() {
    let history = ["git stauts", "cargo test", "ls -la"];
    let close = |word: &str, cutoff: f32| {
        block_on(has_any_close_matches(word, history.iter().copied(), cutoff))
    };

    // "git status" vs "git stauts" has ratio 0.9.
    assert!(close("git status", 0.9));
    assert!(!close("git status", 0.95));
    // Equal lengths pass the quick bound, but no characters match.
    assert!(!close("wxyz", 0.5));
    // "ls" vs "ls -la" has a quick bound and ratio of 0.5.
    assert!(close("ls", 0.5));
    assert!(!close("ls", 0.6));
    // "cargo build" passes the quick bound against "cargo test" (20/21) but
    // its ratio is only 4/7.
    assert!(!close("cargo build", 0.6));
    assert!(close("cargo build", 0.57));
    assert!(!block_on(has_any_close_matches(
        "anything",
        std::iter::empty(),
        0.0,
    )));
}
