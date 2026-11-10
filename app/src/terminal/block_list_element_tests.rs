use super::format_kb_or_mb;

/// Expected strings are what byte-unit 5.2's `{:#.1}` printed for the same values (checked
/// against 2 million values before byte-unit was dropped).
#[test]
fn format_kb_or_mb_matches_byte_unit() {
    for (bytes, expected) in [
        (0, "0 KB"),
        (999, "1 KB"),
        (1_050, "1.1 KB"),
        (12_345, "12.3 KB"),
        (999_950, "1000 KB"),
        (1_000_000, "1 MB"),
        (1_234_567, "1.2 MB"),
        (12_345_678, "12.3 MB"),
    ] {
        assert_eq!(format_kb_or_mb(bytes), expected, "{bytes} bytes");
    }
}
