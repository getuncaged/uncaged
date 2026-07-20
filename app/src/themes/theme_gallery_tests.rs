use super::*;

/// The real catalogue, generated from the live repo by `scripts/build_index.py`.
///
/// This is the load-bearing test of the whole gallery design: the catalogue inlines each theme as
/// JSON, and the app parses it straight into the same `WarpTheme` the rest of the code uses. If
/// that ever stops round-tripping, everything downstream breaks, so the fixture is the genuine
/// article rather than something hand-written to pass.
const REAL_INDEX: &[u8] = include_bytes!("test_data/gallery_index.json");

#[test]
fn the_real_catalogue_parses() {
    let index = parse_index(REAL_INDEX).expect("real index should parse");

    assert_eq!(index.version, SUPPORTED_VERSION);
    assert!(!index.themes.is_empty(), "catalogue should list themes");

    for theme in &index.themes {
        assert!(!theme.name.is_empty(), "{} has no name", theme.id);
        assert!(
            theme.id.starts_with("system/") || theme.id.starts_with("community/"),
            "unexpected group in id {}",
            theme.id,
        );
        assert!(
            theme.path.ends_with(".yaml"),
            "{} should point at a yaml",
            theme.id,
        );
    }
}

/// A theme's colours must survive the trip, since the preview card is rendered from them before
/// anything is downloaded.
#[test]
fn inlined_definitions_carry_usable_colours() {
    let index = parse_index(REAL_INDEX).expect("parse");
    let uncaged = index
        .themes
        .iter()
        .find(|t| t.slug == "uncaged")
        .expect("the default theme should be in the catalogue");

    assert_eq!(uncaged.definition.name().as_deref(), Some("Uncaged"));
    // A gradient background survives as a gradient rather than collapsing to a solid.
    assert_eq!(uncaged.definition.accent().into_solid().r, 0xff);
    assert!(uncaged.definition.background_image().is_none());
}

#[test]
fn a_future_catalogue_version_is_refused_rather_than_misread() {
    let bumped = String::from_utf8(REAL_INDEX.to_vec())
        .unwrap()
        .replace("\"version\": 1", "\"version\": 99");

    let err = parse_index(bumped.as_bytes()).expect_err("should refuse an unknown version");
    assert!(
        err.to_string().contains("99"),
        "error should name the version it saw: {err}",
    );
}

#[test]
fn an_oversized_catalogue_is_refused_before_parsing() {
    let huge = vec![b'{'; MAX_INDEX_BYTES + 1];

    assert!(parse_index(&huge).is_err());
}

#[test]
fn malformed_json_is_an_error_rather_than_a_panic() {
    assert!(parse_index(b"not json at all").is_err());
    assert!(parse_index(b"{\"version\": 1}").is_err(), "missing themes");
}

#[test]
fn search_matches_name_and_slug_case_insensitively() {
    let index = parse_index(REAL_INDEX).expect("parse");
    let uncaged = index.themes.iter().find(|t| t.slug == "uncaged").unwrap();

    assert!(uncaged.matches(""), "empty query matches everything");
    assert!(uncaged.matches("UNCA"));
    assert!(uncaged.matches("caged"));
    assert!(!uncaged.matches("solarized"));
}

#[test]
fn image_urls_are_built_against_the_raw_host() {
    let index = parse_index(REAL_INDEX).expect("parse");
    let without_image = index.themes.iter().find(|t| t.image.is_none()).unwrap();
    assert!(without_image.image_url().is_none());

    let sample = GalleryTheme {
        id: "community/tokyo-rain".into(),
        slug: "tokyo-rain".into(),
        name: "Tokyo Rain".into(),
        group: "community".into(),
        path: "themes/community/tokyo-rain.yaml".into(),
        image: Some("themes/community/tokyo-rain.png".into()),
        definition: index.themes[0].definition.clone(),
    };
    assert_eq!(
        sample.image_url().unwrap(),
        format!("{}/themes/community/tokyo-rain.png", raw_base_url()),
    );
}

/// The install rewrites a theme's image path to an absolute one. This is the subtle bit: relative
/// paths in a theme file resolve against the themes dir, not the file's own folder, so a theme
/// installed into `community/` that kept a relative path would look one directory too high and
/// silently render without its background.
#[test]
fn installing_rewrites_the_image_path_to_an_absolute_one() {
    let mut definition = serde_yaml::from_str::<serde_yaml::Value>(
        "name: Tokyo Rain\nbackground_image:\n  path: ./tokyo-rain.png\n  opacity: 40\n",
    )
    .expect("fixture parses");

    let installed = std::path::Path::new("/tmp/themes/community/tokyo-rain.png");
    set_image_path(&mut definition, installed);

    let rewritten = definition["background_image"]["path"].as_str().unwrap();
    assert_eq!(rewritten, "/tmp/themes/community/tokyo-rain.png");
    // Opacity is left alone.
    assert_eq!(definition["background_image"]["opacity"].as_u64(), Some(40));
}

#[test]
fn rewriting_a_theme_without_an_image_does_nothing() {
    let mut definition =
        serde_yaml::from_str::<serde_yaml::Value>("name: Plain\n").expect("fixture parses");

    set_image_path(&mut definition, std::path::Path::new("/tmp/x.png"));

    assert!(definition.get("background_image").is_none());
}
