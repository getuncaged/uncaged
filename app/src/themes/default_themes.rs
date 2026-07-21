use asset_macro::bundled_or_fetched_asset;
use pathfinder_color::ColorU;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::color::{coloru_with_opacity, OPAQUE};
use warp_core::ui::theme::color::CustomDetails;
use warp_core::ui::theme::{
    AnsiColor, AnsiColors, Details, Fill, HorizontalGradient, Image, TerminalColors,
    VerticalGradient, WarpTheme,
};
use warpui::assets::asset_cache::AssetSource;

const DARK_MODE_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x616161FF),
    AnsiColor::from_u32(0xFF8272FF),
    AnsiColor::from_u32(0xB4FA72FF),
    AnsiColor::from_u32(0xFEFDC2FF),
    AnsiColor::from_u32(0xA5D5FEFF),
    AnsiColor::from_u32(0xFF8FFDFF),
    AnsiColor::from_u32(0xD0D1FEFF),
    AnsiColor::from_u32(0xF1F1F1FF),
);
const DARK_MODE_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x8E8E8EFF),
    AnsiColor::from_u32(0xFFC4BDFF),
    AnsiColor::from_u32(0xD6FCB9FF),
    AnsiColor::from_u32(0xFEFDD5FF),
    AnsiColor::from_u32(0xC1E3FEFF),
    AnsiColor::from_u32(0xFFB1FEFF),
    AnsiColor::from_u32(0xE5E6FEFF),
    AnsiColor::from_u32(0xFEFFFFFF),
);

const LIGHT_MODE_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x212121FF),
    AnsiColor::from_u32(0xC30771FF),
    AnsiColor::from_u32(0x10A778FF),
    AnsiColor::from_u32(0xA89C14FF),
    AnsiColor::from_u32(0x008EC4FF),
    AnsiColor::from_u32(0x523C79FF),
    AnsiColor::from_u32(0x20A5BAFF),
    AnsiColor::from_u32(0xE0E0E0FF),
);
const LIGHT_MODE_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x212121FF),
    AnsiColor::from_u32(0xFB007AFF),
    AnsiColor::from_u32(0x5FD7AFFF),
    AnsiColor::from_u32(0xF3E430FF),
    AnsiColor::from_u32(0x20BBFCFF),
    AnsiColor::from_u32(0x6855DEFF),
    AnsiColor::from_u32(0x4FB8CCFF),
    AnsiColor::from_u32(0xF1F1F1FF),
);

const SOLARIZED_DARK_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x073642FF),
    AnsiColor::from_u32(0xDC322FFF),
    AnsiColor::from_u32(0x859900FF),
    AnsiColor::from_u32(0xB58900FF),
    AnsiColor::from_u32(0x268BD2FF),
    AnsiColor::from_u32(0xD33682FF),
    AnsiColor::from_u32(0x2AA198FF),
    AnsiColor::from_u32(0xEEE8D5FF),
);
const SOLARIZED_DARK_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x002B36FF),
    AnsiColor::from_u32(0xCB4B16FF),
    AnsiColor::from_u32(0x586E75FF),
    AnsiColor::from_u32(0x657B83FF),
    AnsiColor::from_u32(0x839496FF),
    AnsiColor::from_u32(0x6C71C4FF),
    AnsiColor::from_u32(0x93A1A1FF),
    AnsiColor::from_u32(0xFDF6E3FF),
);

const SOLARIZED_LIGHT_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x073642FF),
    AnsiColor::from_u32(0xDC322FFF),
    AnsiColor::from_u32(0x859900FF),
    AnsiColor::from_u32(0xB58900FF),
    AnsiColor::from_u32(0x268BD2FF),
    AnsiColor::from_u32(0xD33682FF),
    AnsiColor::from_u32(0x2AA198FF),
    AnsiColor::from_u32(0xEEE8D5FF),
);
const SOLARIZED_LIGHT_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x002B36FF),
    AnsiColor::from_u32(0xCB4B16FF),
    AnsiColor::from_u32(0x586E75FF),
    AnsiColor::from_u32(0x657B83FF),
    AnsiColor::from_u32(0x839496FF),
    AnsiColor::from_u32(0x6C71C4FF),
    AnsiColor::from_u32(0x93A1A1FF),
    AnsiColor::from_u32(0xFDF6E3FF),
);

const DRACULA_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x000000FF),
    AnsiColor::from_u32(0xFF5555FF),
    AnsiColor::from_u32(0x50FA7BFF),
    AnsiColor::from_u32(0xF1FA8CFF),
    AnsiColor::from_u32(0xBD93F9FF),
    AnsiColor::from_u32(0xFF79C6FF),
    AnsiColor::from_u32(0x8BE9FDFF),
    AnsiColor::from_u32(0xBBBBBBFF),
);
const DRACULA_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x555555FF),
    AnsiColor::from_u32(0xFF5555FF),
    AnsiColor::from_u32(0x50FA7BFF),
    AnsiColor::from_u32(0xF1FA8CFF),
    AnsiColor::from_u32(0xCAA9FAFF),
    AnsiColor::from_u32(0xFF79C6FF),
    AnsiColor::from_u32(0x8BE9FDFF),
    AnsiColor::from_u32(0xFFFFFFFF),
);

const PHENOMENON_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x121212FF),
    AnsiColor::from_u32(0xD22D1EFF),
    AnsiColor::from_u32(0x1CA05AFF),
    AnsiColor::from_u32(0xE5A01AFF),
    AnsiColor::from_u32(0x3780E9FF),
    AnsiColor::from_u32(0xBF409DFF),
    AnsiColor::from_u32(0x799C92FF),
    AnsiColor::from_u32(0xFAF9F6FF),
);
const PHENOMENON_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x292929FF),
    AnsiColor::from_u32(0xAE756FFF),
    AnsiColor::from_u32(0x789B88FF),
    AnsiColor::from_u32(0xBD9F65FF),
    AnsiColor::from_u32(0x6F839FFF),
    AnsiColor::from_u32(0xA57899FF),
    AnsiColor::from_u32(0xBFC5C3FF),
    AnsiColor::from_u32(0xFFFFFFFF),
);

const GRUVBOX_DARK_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x282828FF),
    AnsiColor::from_u32(0xCC241DFF),
    AnsiColor::from_u32(0x98971AFF),
    AnsiColor::from_u32(0xD79921FF),
    AnsiColor::from_u32(0x458588FF),
    AnsiColor::from_u32(0xB16286FF),
    AnsiColor::from_u32(0x689D6AFF),
    AnsiColor::from_u32(0xA89984FF),
);
const GRUVBOX_DARK_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x928374FF),
    AnsiColor::from_u32(0xFB4934FF),
    AnsiColor::from_u32(0xB8BB26FF),
    AnsiColor::from_u32(0xFABD2FFF),
    AnsiColor::from_u32(0x83A598FF),
    AnsiColor::from_u32(0xD3869BFF),
    AnsiColor::from_u32(0x8EC07CFF),
    AnsiColor::from_u32(0xEBDBB2FF),
);

const GRUVBOX_LIGHT_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0xFBF1C7FF),
    AnsiColor::from_u32(0xCC241DFF),
    AnsiColor::from_u32(0x98971AFF),
    AnsiColor::from_u32(0xD79921FF),
    AnsiColor::from_u32(0x458588FF),
    AnsiColor::from_u32(0xB16286FF),
    AnsiColor::from_u32(0x689D6AFF),
    AnsiColor::from_u32(0x7C6F64FF),
);
const GRUVBOX_LIGHT_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x928374FF),
    AnsiColor::from_u32(0x9D0006FF),
    AnsiColor::from_u32(0x79740EFF),
    AnsiColor::from_u32(0xB57614FF),
    AnsiColor::from_u32(0x076678FF),
    AnsiColor::from_u32(0x8F3F71FF),
    AnsiColor::from_u32(0x427B58FF),
    AnsiColor::from_u32(0x3C3836FF),
);

const SOLARFLARE_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x2E333DFF),
    AnsiColor::from_u32(0xD66060FF),
    AnsiColor::from_u32(0x64AF86FF),
    AnsiColor::from_u32(0xCAA358FF),
    AnsiColor::from_u32(0x5C80B2FF),
    AnsiColor::from_u32(0xB766A1FF),
    AnsiColor::from_u32(0x8069A1FF),
    AnsiColor::from_u32(0xF0F4F7FF),
);
const SOLARFLARE_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x37404AFF),
    AnsiColor::from_u32(0xEB8282FF),
    AnsiColor::from_u32(0x64AF86FF),
    AnsiColor::from_u32(0xCAA358FF),
    AnsiColor::from_u32(0x5C80B2FF),
    AnsiColor::from_u32(0xB766A1FF),
    AnsiColor::from_u32(0x8069A1FF),
    AnsiColor::from_u32(0xFFFFFFFF),
);

const ADEBERRY_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x121212FF),
    AnsiColor::from_u32(0xC76156FF),
    AnsiColor::from_u32(0x57C78AFF),
    AnsiColor::from_u32(0xC8A35AFF),
    AnsiColor::from_u32(0x5785C7FF),
    AnsiColor::from_u32(0xC756A9FF),
    AnsiColor::from_u32(0x57C7C3FF),
    AnsiColor::from_u32(0xEEEDEBFF),
);
const ADEBERRY_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x292929FF),
    AnsiColor::from_u32(0xD22D1EFF),
    AnsiColor::from_u32(0x1CA05AFF),
    AnsiColor::from_u32(0xE5A01AFF),
    AnsiColor::from_u32(0x1458B8FF),
    AnsiColor::from_u32(0xA43787FF),
    AnsiColor::from_u32(0x4D9989FF),
    AnsiColor::from_u32(0xFFFFFFFF),
);

pub(super) fn light_mode_colors() -> TerminalColors {
    TerminalColors::new(LIGHT_MODE_NORMAL_COLORS, LIGHT_MODE_BRIGHT_COLORS)
}

pub(super) fn dark_mode_colors() -> TerminalColors {
    TerminalColors::new(DARK_MODE_NORMAL_COLORS, DARK_MODE_BRIGHT_COLORS)
}

pub(super) fn solarized_light_colors() -> TerminalColors {
    TerminalColors::new(SOLARIZED_LIGHT_NORMAL_COLORS, SOLARIZED_LIGHT_BRIGHT_COLORS)
}

pub(super) fn solarized_dark_colors() -> TerminalColors {
    TerminalColors::new(SOLARIZED_DARK_NORMAL_COLORS, SOLARIZED_DARK_BRIGHT_COLORS)
}

pub(super) fn dracula_colors() -> TerminalColors {
    TerminalColors::new(DRACULA_NORMAL_COLORS, DRACULA_BRIGHT_COLORS)
}

pub(super) fn phenomenon_colors() -> TerminalColors {
    TerminalColors::new(PHENOMENON_NORMAL_COLORS, PHENOMENON_BRIGHT_COLORS)
}

pub(super) fn gruvbox_dark_colors() -> TerminalColors {
    TerminalColors::new(GRUVBOX_DARK_NORMAL_COLORS, GRUVBOX_DARK_BRIGHT_COLORS)
}

pub(super) fn gruvbox_light_colors() -> TerminalColors {
    TerminalColors::new(GRUVBOX_LIGHT_NORMAL_COLORS, GRUVBOX_LIGHT_BRIGHT_COLORS)
}

pub(super) fn solarflare_colors() -> TerminalColors {
    TerminalColors::new(SOLARFLARE_NORMAL_COLORS, SOLARFLARE_BRIGHT_COLORS)
}

pub(super) fn adeberry_colors() -> TerminalColors {
    TerminalColors::new(ADEBERRY_NORMAL_COLORS, ADEBERRY_BRIGHT_COLORS)
}

/// Default bundled themes
pub fn dark_theme() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x000000FF)),
        ColorU::from_u32(0xffffffff),
        Fill::Solid(ColorU::from_u32(0x19AAD8FF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        None,
        Some("Dark".to_string()),
    )
}

pub fn light_theme() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::white()),
        ColorU::new(17, 17, 17, OPAQUE),
        Fill::Solid(ColorU::from_u32(0x00c2ffff)),
        None,
        Some(Details::Lighter),
        light_mode_colors(),
        None,
        Some("Light".to_string()),
    )
}

pub(super) fn dracula() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x282A36FF)),
        ColorU::from_u32(0xF8F8F2FF),
        Fill::Solid(ColorU::from_u32(0xFF79C6FF)),
        None,
        Some(Details::Darker),
        dracula_colors(),
        None,
        Some("Dracula".to_string()),
    )
}

pub(super) fn solarized_light() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0xFDF6E3FF)),
        ColorU::from_u32(0x586E75FF),
        Fill::Solid(ColorU::from_u32(0x66B5A9FF)),
        None,
        Some(Details::Lighter),
        solarized_light_colors(),
        None,
        Some("Solarized Light".to_string()),
    )
}

pub(super) fn solarized_dark() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x002B36FF)),
        ColorU::from_u32(0xF8F8F2FF),
        Fill::Solid(ColorU::from_u32(0xCB4B16FF)),
        None,
        Some(Details::Darker),
        solarized_dark_colors(),
        None,
        Some("Solarized Dark".to_string()),
    )
}

pub(super) fn gruvbox_dark() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x282828FF)),
        ColorU::from_u32(0xEBDBB2FF),
        Fill::Solid(ColorU::from_u32(0xFC802DFF)),
        None,
        Some(Details::Darker),
        gruvbox_dark_colors(),
        None,
        Some("Gruvbox Dark".to_string()),
    )
}

pub(super) fn gruvbox_light() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0xFBF1C7FF)),
        ColorU::from_u32(0x3C3836FF),
        Fill::Solid(ColorU::from_u32(0xAD3B14FF)),
        None,
        Some(Details::Lighter),
        gruvbox_light_colors(),
        None,
        Some("Gruvbox Light".to_string()),
    )
}

/// Bundled gradient themes
pub(super) fn cyber_wave() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::black().blend(&coloru_with_opacity(ColorU::from_u32(0x00C2FFFF), 20)),
            ColorU::black(),
        )),
        ColorU::white(),
        Fill::HorizontalGradient(HorizontalGradient::new(
            ColorU::from_u32(0x007972FF),
            ColorU::from_u32(0x7B008FFF),
        )),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        None,
        Some("Cyber Wave".to_string()),
    )
}

pub(super) fn willow_dream() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x206169FF),
            ColorU::from_u32(0x022F27FF),
        )),
        ColorU::white(),
        Fill::HorizontalGradient(HorizontalGradient::new(
            ColorU::from_u32(0xF9AEA8FF),
            ColorU::from_u32(0xDD6258FF),
        )),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        None,
        Some("Willow Dream".to_string()),
    )
}

pub(super) fn fancy_dracula() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x252630FF),
            ColorU::from_u32(0x3D3F4FFF),
        )),
        ColorU::white(),
        Fill::HorizontalGradient(HorizontalGradient::new(
            ColorU::from_u32(0xBCA1F6FF),
            ColorU::from_u32(0xA3E7FCFF),
        )),
        None,
        Some(Details::Darker),
        dracula_colors(),
        None,
        Some("Fancy Dracula".to_string()),
    )
}

pub(super) fn phenomenon() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x121212FF)),
        ColorU::from_u32(0xFAF9F6FF),
        Fill::Solid(ColorU::from_u32(0x2E5D9EFF)),
        None,
        Some(Details::Darker),
        phenomenon_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/phenomenon_bg.jpg"),
            opacity: 100,
        }),
        Some("Phenomenon".to_string()),
    )
}

/// Bundled themes with background images
pub(super) fn jellyfish() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x1B1718FF)),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0x538682FF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/jellyfish_bg.jpg"),
            opacity: 30,
        }),
        Some("Jellyfish".to_string()),
    )
}

pub(super) fn koi() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x211719FF)),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0xFF3131FF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/koi_bg.jpg"),
            opacity: 30,
        }),
        Some("Koi".to_string()),
    )
}

pub(super) fn leafy() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::black()),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0x55972DFF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/leafy_bg.jpg"),
            opacity: 30,
        }),
        Some("Leafy".to_string()),
    )
}

pub(super) fn marble() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0xE3E3E3FF)),
        ColorU::black(),
        Fill::Solid(ColorU::from_u32(0x585858FF)),
        None,
        Some(Details::Lighter),
        light_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/marble_bg.jpg"),
            opacity: 50,
        }),
        Some("Marble".to_string()),
    )
}

pub(super) fn pink_city() -> WarpTheme {
    let details = CustomDetails {
        ..CustomDetails::lighter_details()
    };
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0xFBEFF6FF)),
        ColorU::black(),
        Fill::Solid(ColorU::from_u32(0xE10087FF)),
        None,
        Some(Details::Custom(details)),
        light_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/pink_city_bg.jpg"),
            opacity: 40,
        }),
        Some("Pink City".to_string()),
    )
}

pub(super) fn snowy() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0xFFFFFFFF),
            ColorU::from_u32(0xDEE6EBFF),
        )),
        ColorU::black(),
        Fill::Solid(ColorU::from_u32(0x647E90FF)),
        None,
        Some(Details::Lighter),
        light_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/snowy_bg.jpg"),
            opacity: 20,
        }),
        Some("Snowy".to_string()),
    )
}

pub(super) fn red_rock() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x211719FF)
                .blend(&coloru_with_opacity(ColorU::from_u32(0x4C3435FF), 45)),
            ColorU::from_u32(0x211719FF)
                .blend(&coloru_with_opacity(ColorU::from_u32(0xD3032FF), 45)),
        )),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0x9F4147FF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/red_rock_bg.jpg"),
            opacity: 30,
        }),
        Some("Red Rock".to_string()),
    )
}

pub(super) fn dark_city() -> WarpTheme {
    WarpTheme::new(
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x01181FFF)
                .blend(&coloru_with_opacity(ColorU::from_u32(0x1A363FFF), 45)),
            ColorU::from_u32(0x01181FFF)
                .blend(&coloru_with_opacity(ColorU::from_u32(0x1A4551FF), 45)),
        )),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0xE9072DFF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/dark_city_bg.jpg"),
            opacity: 20,
        }),
        Some("Dark City".to_string()),
    )
}

pub(super) fn sent_referral_reward() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x334567FF)),
        ColorU::white(),
        Fill::Solid(ColorU::from_u32(0xCD51FFFF)),
        None,
        Some(Details::Darker),
        dark_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/sent_referral_reward_bg.jpg"),
            opacity: 100,
        }),
        Some("Warp Referral".to_string()),
    )
}

pub(super) fn solar_flare() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x1B1C18FF)),
        ColorU::from_u32(0xDDE6EEFF),
        Fill::Solid(ColorU::from_u32(0x34895CFF)),
        None,
        Some(Details::Darker),
        solarflare_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/solarflare_bg.jpg"),
            opacity: 20,
        }),
        Some("Solar Flare".to_string()),
    )
}

pub(super) fn adeberry() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0x1D2022FF)),
        ColorU::from_u32(0xE4EEF5FF),
        Fill::Solid(ColorU::from_u32(0x6C96B4FF)),
        None,
        Some(Details::Darker),
        adeberry_colors(),
        None,
        Some("Adeberry".to_string()),
    )
}

// ── Uncaged ────────────────────────────────────────────────────────────────
// The out-of-the-box default. A warm "night-workbench" ground lit by ember
// accents, tuned to the [ ❯_ ] mark: gold→red ignition on a deep, warm
// near-black. Palette knobs live in `app/src/brand.rs`.
const UNCAGED_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x4A443BFF), // black   — warm graphite
    AnsiColor::from_u32(0xFF6B5EFF), // red     — coral ember
    AnsiColor::from_u32(0x8FD46EFF), // green   — warm leaf
    AnsiColor::from_u32(0xFFC24AFF), // yellow  — gold
    AnsiColor::from_u32(0x74B4E0FF), // blue    — soft sky
    AnsiColor::from_u32(0xF090C0FF), // magenta — warm rose
    AnsiColor::from_u32(0x5FC9BEFF), // cyan    — teal
    AnsiColor::from_u32(0xE9E1D4FF), // white   — warm ink
);
const UNCAGED_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x6E655AFF), // bright black
    AnsiColor::from_u32(0xFF8A76FF), // bright red
    AnsiColor::from_u32(0xB2E38CFF), // bright green
    AnsiColor::from_u32(0xFFD46EFF), // bright yellow
    AnsiColor::from_u32(0x9BCBEEFF), // bright blue
    AnsiColor::from_u32(0xF7ABD6FF), // bright magenta
    AnsiColor::from_u32(0x86DED0FF), // bright cyan
    AnsiColor::from_u32(0xFBF6ECFF), // bright white
);
pub(super) fn uncaged_colors() -> TerminalColors {
    TerminalColors::new(UNCAGED_NORMAL_COLORS, UNCAGED_BRIGHT_COLORS)
}

pub(super) fn uncaged() -> WarpTheme {
    WarpTheme::new(
        // Warm near-black ground, with a faint glow lifting from the top.
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x15110CFF),
            ColorU::from_u32(0x0C0A08FF),
        )),
        ColorU::from_u32(0xE9E1D4FF),              // warm ink foreground
        Fill::Solid(ColorU::from_u32(0xFF7A18FF)), // ember accent
        // A bright gold caret — the ember that lights the [ ❯_ ] mark.
        Some(Fill::Solid(ColorU::from_u32(0xFFB23AFF))),
        Some(Details::Darker),
        uncaged_colors(),
        None,
        Some("Uncaged".to_string()),
    )
}

// ── Midnight ─────────────────────────────────────────────────────────────
// The cold companion to Uncaged: a disciplined blue-black night where the only
// warmth is the same ember. Mirrors getuncaged.dev's "Midnight" web system —
// cool bases, porcelain text, gold→ember reserved for action and the mark.
const MIDNIGHT_NORMAL_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x363E4FFF), // black   — cool slate
    AnsiColor::from_u32(0xF06A78FF), // red     — cool coral
    AnsiColor::from_u32(0x7BD88FFF), // green   — mint (diff-added)
    AnsiColor::from_u32(0xFFB23AFF), // yellow  — gold (the one warm pop)
    AnsiColor::from_u32(0x6AA8FFFF), // blue    — steel
    AnsiColor::from_u32(0xC8A9F0FF), // magenta — cool lavender
    AnsiColor::from_u32(0x5FC7D6FF), // cyan    — cool cyan
    AnsiColor::from_u32(0xC7CCD6FF), // white   — cool light gray
);
const MIDNIGHT_BRIGHT_COLORS: AnsiColors = AnsiColors::new(
    AnsiColor::from_u32(0x5D6577FF), // bright black   — muted slate
    AnsiColor::from_u32(0xFF8089FF), // bright red     — soft coral (diff-removed)
    AnsiColor::from_u32(0xA5E4B4FF), // bright green
    AnsiColor::from_u32(0xFFC96BFF), // bright yellow
    AnsiColor::from_u32(0x8FC1FFFF), // bright blue
    AnsiColor::from_u32(0xDCC2FFFF), // bright magenta
    AnsiColor::from_u32(0x86DCE8FF), // bright cyan
    AnsiColor::from_u32(0xE8EBF2FF), // bright white   — porcelain (the ceiling)
);
pub(super) fn midnight_colors() -> TerminalColors {
    TerminalColors::new(MIDNIGHT_NORMAL_COLORS, MIDNIGHT_BRIGHT_COLORS)
}

pub(super) fn midnight() -> WarpTheme {
    WarpTheme::new(
        // Cool blue-black ground with a faint slate lift from the top.
        Fill::VerticalGradient(VerticalGradient::new(
            ColorU::from_u32(0x12151DFF),
            ColorU::from_u32(0x0B0D12FF),
        )),
        ColorU::from_u32(0xE8EBF2FF), // porcelain ink foreground
        Fill::Solid(ColorU::from_u32(0xFF7A18FF)), // ember accent — the only warmth
        // Gold caret — the same grad-prompt head that lights the [ ❯_ ] mark.
        Some(Fill::Solid(ColorU::from_u32(0xFFB23AFF))),
        Some(Details::Darker),
        midnight_colors(),
        None,
        Some("Midnight".to_string()),
    )
}

pub(super) fn received_referral_reward() -> WarpTheme {
    WarpTheme::new(
        Fill::Solid(ColorU::from_u32(0xFFFFFFFF)),
        ColorU::black(),
        Fill::Solid(ColorU::from_u32(0xCD51FFFF)),
        None,
        Some(Details::Lighter),
        light_mode_colors(),
        Some(Image {
            source: bundled_or_fetched_asset!("jpg/received_referral_reward_bg.jpg"),
            opacity: 100,
        }),
        Some("Received Referral Reward".to_string()),
    )
}

/// Swaps a bundled theme background for its pre-scaled preview thumbnail.
///
/// A theme row in the picker is 190x100, but `background_image()` points at the full-size
/// wallpaper — 2048x1536 and up. Decoding one of those and downscaling it with a Triangle filter
/// costs 13-32ms, and both run synchronously inside `Image::paint`, so the cost lands on the frame
/// each row is first revealed and the list stutters as it scrolls. The thumbnails under
/// `jpg/thumbs/` are the same pictures at 380x200 — about 1/40th the pixels, and 124KB against
/// 7.9MB for the originals.
///
/// This lives here, and goes through the macro rather than matching on path strings, because
/// `build.rs` rewrites async assets to content-hashed names (`koi_bg-<sha256>.jpg`). A hand-written
/// path would never resolve, and would fail silently as a blank preview; the macro computes the
/// same hash at compile time, so a missing thumbnail is a build error instead.
///
/// Anything without a thumbnail — including every user-created theme, which is a `LocalFile` —
/// falls through to its original source and behaves exactly as before.
pub fn preview_source(source: AssetSource) -> AssetSource {
    // The macro needs real string literals (it hashes the file at compile time), so each pair is
    // spelled out rather than built with `concat!`.
    macro_rules! thumbnail_for {
        ($source:expr, $($full:literal => $thumb:literal),+ $(,)?) => {
            $(
                if $source == bundled_or_fetched_asset!($full) {
                    return bundled_or_fetched_asset!($thumb);
                }
            )+
        };
    }

    thumbnail_for!(
        source,
        "jpg/dark_city_bg.jpg" => "jpg/thumbs/dark_city_bg.jpg",
        "jpg/jellyfish_bg.jpg" => "jpg/thumbs/jellyfish_bg.jpg",
        "jpg/koi_bg.jpg" => "jpg/thumbs/koi_bg.jpg",
        "jpg/leafy_bg.jpg" => "jpg/thumbs/leafy_bg.jpg",
        "jpg/marble_bg.jpg" => "jpg/thumbs/marble_bg.jpg",
        "jpg/phenomenon_bg.jpg" => "jpg/thumbs/phenomenon_bg.jpg",
        "jpg/pink_city_bg.jpg" => "jpg/thumbs/pink_city_bg.jpg",
        "jpg/received_referral_reward_bg.jpg" => "jpg/thumbs/received_referral_reward_bg.jpg",
        "jpg/red_rock_bg.jpg" => "jpg/thumbs/red_rock_bg.jpg",
        "jpg/sent_referral_reward_bg.jpg" => "jpg/thumbs/sent_referral_reward_bg.jpg",
        "jpg/snowy_bg.jpg" => "jpg/thumbs/snowy_bg.jpg",
        "jpg/solarflare_bg.jpg" => "jpg/thumbs/solarflare_bg.jpg",
    );

    // A theme loaded from disk — downloaded or user-made — carries its image as a LocalFile. The
    // importer writes a `<name>.thumb.jpg` next to the full image, so a card can decode that
    // instead of a 2560px original. This is the difference between a ~0.3MB preview and ~15MB of
    // decoded RGBA held resident for the process's life, since theme-image sources are never
    // evicted from the asset cache.
    //
    // The `exists()` check keeps two things honest: a theme installed before thumbnails existed
    // still previews (it falls back to the full image), and the check is cheap because a preview
    // grid only repaints on interaction, not per frame. Only paths inside the themes dir are
    // touched — a theme may point at a wallpaper elsewhere, which is not ours to look beside.
    if let AssetSource::LocalFile { path, .. } = &source {
        let full = std::path::Path::new(path);
        if full.starts_with(crate::user_config::themes_dir()) {
            let thumb = crate::themes::theme_background_image::thumbnail_path(full);
            if thumb.exists() {
                return AssetSource::LocalFile {
                    path: thumb.to_string_lossy().into_owned(),
                    content_version: None,
                };
            }
        }
    }

    source
}
