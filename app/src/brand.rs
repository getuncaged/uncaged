//! # Uncaged brand — the single source of truth.
//!
//! This is the ONE place to rebrand and re-skin the product. Forking and making
//! it your own should touch exactly three locations, all pointed to from here:
//!
//! 1. **This file** — name, tagline, URLs, the accent/ground **palette**, the
//!    About description + community label, and the [`design`] module: every
//!    glyph the app uses for its own surfaces (the cage [`design::MARK`], the
//!    ember-caret [`design::AGENT`], and the rail icons `DRIVE`/`SSH`/`SKILLS`/
//!    `CONFIG`). Change an icon here and the whole UI follows.
//! 2. **`app/assets/bundled/svg/brand/`** — the logo art: `uncaged-mark.svg`
//!    (the in-app monochrome cage glyph, `#FF0000` recolour sentinel),
//!    `ai-caret.svg` (the ember AI caret), and `uncaged-icon.svg` (the colour
//!    app-icon master → `.icns` / PNGs). The About wordmark lives at
//!    `uncaged-logo-{light,dark}-title.svg`.
//! 3. **`crates/uncaged_engine/src/catalog.rs`** — the AI providers/tools the
//!    "Connect a model" gallery offers.
//!
//! Nothing else should hard-code the name, colours, logo path, or UI icons;
//! reach for the constants here instead so a rebrand stays a one-file change.

#![allow(dead_code)]

use warpui::color::ColorU;

/// Product name, as shown in every user-facing surface.
pub const NAME: &str = "Uncaged";

/// Lowercase machine form (config dirs, ids, urls).
pub const NAME_LOWER: &str = "uncaged";

/// One-line description of what the product is.
pub const TAGLINE: &str = "No account. No cloud. No cage.";

/// The secondary line — what you get in exchange.
pub const SUBLINE: &str = "Your terminal. Your models. Uncaged.";

/// Primary domain (docs, marketing, downloads).
pub const DOMAIN: &str = "getuncaged.dev";

/// The project homepage on the web.
pub const WEBSITE_URL: &str = "https://getuncaged.dev";

/// The GitHub organization that owns the project.
pub const GITHUB_ORG: &str = "github.com/getuncaged";

/// Builds a repository URL from a path/fragment suffix at compile time. This is
/// the ONE place the repo base lives — a fork changes this single literal and
/// every in-app link (home, issues, discussions, docs) follows. Nothing else in
/// the codebase should hard-code the repo URL; reach for the constants below.
macro_rules! repo_url {
    ($suffix:literal) => {
        concat!("https://github.com/getuncaged/uncaged", $suffix)
    };
}

/// Project home (docs, issues, source).
pub const HOME_URL: &str = repo_url!("");
/// The repository README — the app's generic "learn more" destination.
pub const README_URL: &str = repo_url!("#readme");
/// The issue tracker.
pub const ISSUES_URL: &str = repo_url!("/issues");
/// The "open a new issue" form.
pub const NEW_ISSUE_URL: &str = repo_url!("/issues/new");
/// Community discussions.
pub const DISCUSSIONS_URL: &str = repo_url!("/discussions");
/// The privacy section of the README.
pub const PRIVACY_URL: &str = repo_url!("#privacy");

// ── Community themes repository ────────────────────────────────────────────────
//
// A SEPARATE repo from the main one above: the theme gallery browses it and the
// theme editor opens pull requests against it. The `owner/repo` slug lives once,
// in the macro below; every URL derives from it, and the `theme_gallery` /
// `theme_creator` modules build all their links from these constants rather than
// hard-coding anything. A fork changes the one literal to point at its own gallery.

/// Prefixes/suffixes the community-themes `owner/repo` slug at compile time, so the
/// slug itself appears exactly once. Mirrors [`repo_url!`] for the main repo.
macro_rules! themes_url {
    ($prefix:literal, $suffix:literal) => {
        concat!($prefix, "getuncaged/uncaged-themes", $suffix)
    };
}

/// `owner/repo` of the community themes gallery.
pub const THEMES_REPO_SLUG: &str = themes_url!("", "");

/// The themes repo's web home — where a shared theme's pull request opens.
pub const THEMES_REPO_URL: &str = themes_url!("https://github.com/", "");

/// Base for `raw.githubusercontent.com`, without a trailing ref. Callers append
/// `/{ref}/{path}` to fetch a file's contents.
pub const THEMES_RAW_BASE: &str = themes_url!("https://raw.githubusercontent.com/", "");

/// Base for the GitHub REST API for this repo. Callers append `/pulls`, etc.
pub const THEMES_API_BASE: &str = themes_url!("https://api.github.com/repos/", "");

/// The env var that overrides which gallery branch the app reads, named after the
/// product so a fork's variable matches its own name.
pub const THEME_GALLERY_REF_ENV: &str = "UNCAGED_THEME_GALLERY_REF";

/// The `User-Agent` product token for the app's own HTTP requests, e.g. to GitHub.
/// `concat!` keeps it a compile-time `&'static str`; a fork changes the literal.
pub const HTTP_USER_AGENT: &str = concat!("Uncaged/", env!("CARGO_PKG_VERSION"));

// ── Brand artwork ────────────────────────────────────────────────────────────
//
// The monochrome glyph paths are DEFINED ONCE in `warp_core::ui::icons` (next to
// the `Icon` -> asset mapping, which is the lowest crate every surface can reach)
// and re-exported here so this file stays the one design entry point. Change the
// value there — or swap the SVG — and every surface follows, including the icon
// enum, the slash-command menu, and search filters.

/// Bundled path to the AI / agent caret glyph (recolour sentinel). Re-exported so the
/// design entry point names every brand glyph, even where the app reaches it via `Icon`.
#[allow(unused_imports)]
pub use warp_core::ui::icons::BRAND_AGENT_SVG as AGENT_SVG;
/// Bundled path to the in-app monochrome mark (recolour sentinel).
pub use warp_core::ui::icons::BRAND_MARK_SVG as MARK_SVG;

/// Bundled path to the colour app-icon master.
pub const ICON_SVG: &str = "bundled/svg/brand/uncaged-icon.svg";

/// Full-colour square logos used on the account/auth surfaces. Unlike the
/// monochrome glyphs above these carry their own ember gradient, so they are NOT
/// recolour sentinels and must not be swapped for `MARK_SVG`.
pub const LOGO_LIGHT_SVG: &str = "bundled/svg/warp-logo-light.svg";
/// Dark-background variant of [`LOGO_LIGHT_SVG`].
pub const LOGO_DARK_SVG: &str = "bundled/svg/warp-logo-dark.svg";

/// One-line About-screen description. Factual reference to upstream is on-mission.
pub const ABOUT_DESCRIPTION: &str = "Uncaged — an open source fork of Warp";

/// About-screen source + license line. AGPL-3.0 §13 asks that users of a
/// modified version be offered its complete source; surfacing the source repo
/// and license right in the running app is the belt-and-suspenders way to do
/// that (the repo NOTICE/README carry the full attribution).
pub const ABOUT_SOURCE: &str = "Source: github.com/getuncaged/uncaged · AGPL-3.0";

/// Label for the community/discussions link (app menu, resource center).
pub const COMMUNITY_LABEL: &str = "Community";

/// The in-app iconography — every glyph the app uses for its OWN surfaces, in
/// one place so a fork can re-skin the whole UI from here. Values are
/// `warp_core::ui::Icon` variants; their SVGs live in `app/assets/bundled/svg/`
/// (brand marks under `brand/`). To change a rail/agent icon in a fork, edit
/// the constant here (and/or swap the underlying SVG).
pub mod design {
    use warp_core::ui::Icon;

    /// The app's own mark (window / tab / agent avatar identity): the terminal
    /// cage glyph — `brand/uncaged-mark.svg`.
    ///
    /// `Icon::Oz`, `Icon::OzCloud`, `Icon::Warp` and `Icon::WarpLogoLight` are
    /// upstream variant names that all resolve to this same artwork, so no
    /// surface can render a stale upstream glyph.
    pub const MARK: Icon = Icon::Oz;
    /// The AI / agent mark: the ember ❯ prompt caret (`brand/ai-caret.svg`).
    /// Shown as the agent-session identity and the AI-reply fallback when the
    /// connected provider has no logo of its own.
    pub const AGENT: Icon = Icon::AiCaret;

    // ---- Left tool-panel rail icons ----
    /// Local Drive panel — a database / storage cylinder
    /// (`icons::BRAND_DRIVE_SVG`).
    pub const DRIVE: Icon = Icon::WarpDrive;
    /// SSH hosts panel — a connection link (`link-03.svg`).
    pub const SSH: Icon = Icon::Link;
    /// Skills panel — an open book (`book-open.svg`).
    pub const SKILLS: Icon = Icon::BookOpen;
    /// Config panel — a gear (`gear.svg`).
    pub const CONFIG: Icon = Icon::Gear;
}

/// Build a `ColorU` from a `0xRRGGBB` literal (opaque).
pub fn rgb(hex: u32) -> ColorU {
    ColorU::new(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
        255,
    )
}

/// The ember / ignition accent — the brand's one bold colour, warm on dark.
/// Gold → orange → red, so a gradient reads like a flame catching.
pub mod ember {
    pub const GOLD: u32 = 0xFFCE4E;
    pub const ORANGE: u32 = 0xFF7A18;
    pub const RED: u32 = 0xFF3B47;
    /// The single accent for one-colour contexts (links, focus, the prompt).
    pub const ACCENT: u32 = ORANGE;
}

/// Warm "night-workbench" neutrals for the default theme — a near-black with a
/// slight ember bias, never a dead grey.
pub mod ground {
    pub const BASE: u32 = 0x0E0D0C;
    pub const PANEL: u32 = 0x17140F;
    pub const RAISED: u32 = 0x1F1B15;
    pub const LINE: u32 = 0x2C2620;
    pub const INK: u32 = 0xECE6DC;
    pub const MUTED: u32 = 0x8C8378;
    pub const FAINT: u32 = 0x5A5349;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The community-themes URLs must all derive from the one slug, so a fork that changes
    /// `THEMES_REPO_SLUG` (via the `themes_url!` macro) can trust every link to follow.
    #[test]
    fn theme_repo_urls_all_derive_from_one_slug() {
        assert_eq!(THEMES_REPO_SLUG, "getuncaged/uncaged-themes");
        for url in [THEMES_REPO_URL, THEMES_RAW_BASE, THEMES_API_BASE] {
            assert!(
                url.ends_with(THEMES_REPO_SLUG),
                "{url} should end with the slug {THEMES_REPO_SLUG}",
            );
        }
        assert!(THEMES_REPO_URL.starts_with("https://github.com/"));
        assert!(THEMES_RAW_BASE.starts_with("https://raw.githubusercontent.com/"));
        assert!(THEMES_API_BASE.starts_with("https://api.github.com/repos/"));
    }

    /// The user agent carries the product name and the crate version.
    #[test]
    fn user_agent_names_the_product() {
        assert!(HTTP_USER_AGENT.starts_with("Uncaged/"));
        assert!(HTTP_USER_AGENT.len() > "Uncaged/".len());
    }
}
