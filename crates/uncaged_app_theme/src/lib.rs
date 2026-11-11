//! Leaf theme data for Uncaged: the brand constants and the theme model, built-in themes,
//! background-image helpers and community theme gallery. The views that edit and browse themes
//! (theme creator/explorer) stay in the app crate because they need the editor and settings.

pub mod brand;
pub mod themes;

/// Path to a bundled async jpg asset; the assets live in the app crate's `assets` directory.
#[cfg(test)]
fn async_jpg(name: &str) -> std::path::PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "app",
        "assets",
        "async",
        "jpg",
        name,
    ]
    .iter()
    .collect()
}
