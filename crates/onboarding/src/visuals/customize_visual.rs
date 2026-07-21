use warp_core::ui::appearance::Appearance;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warpui_core::elements::Align;
use warpui_core::Element;

use super::onboarding_visual::{IconPct, OnboardingVisual, Pill, Rect, RectPct};

/// A live, drawn mock of the Uncaged window whose layout mirrors the real app,
/// so each setting reads at a glance (placements copied from the app):
///   * tab styling  -> tabs are a left rail (vertical) with session items, or a
///     top bar (horizontal) of tabs, each a status dot + title;
///   * tools panel  -> a file-TREE sidebar (folders + nested files) appears;
///   * code review  -> a right panel of file-diff CARDS (filename + `+N -N` badge
///     + line-numbered green/red rows), like the real review panel.
/// Around them sits a minimal agent conversation (avatar + prompt + command
/// blocks) so it reads as the agentic terminal.
pub(crate) fn customize_visual(
    appearance: &Appearance,
    use_vertical_tabs: bool,
    tools_enabled: bool,
    code_review: bool,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let window = internal_colors::neutral_2(theme);
    let panel = internal_colors::neutral_1(theme); // recessed rails / panels
    let card = internal_colors::neutral_3(theme); // raised cards on a panel
    let bar = internal_colors::neutral_5(theme);
    let bar_dim = internal_colors::neutral_4(theme);
    let icon_col = internal_colors::neutral_6(theme);
    let accent = internal_colors::accent(theme).into_solid();
    let accent_soft = internal_colors::accent_overlay_1(theme).into_solid();
    let green = theme.ansi_fg_green();
    let red = theme.ansi_fg_red();

    let mut rects: Vec<Rect> = Vec::new();
    let mut pills: Vec<Pill> = Vec::new();
    let mut icons: Vec<IconPct> = Vec::new();

    // ---------- Tab navigation ----------
    let content_top;
    let left_edge;
    if use_vertical_tabs {
        // Left rail of session tabs; each = status dot + title bar + sub bar.
        rects.push(Rect {
            rect: RectPct::new(0.0, 0.0, 0.15, 1.0),
            color: panel,
        });
        for i in 0..3 {
            let y = 0.07 + i as f32 * 0.15;
            let dot = if i == 0 { accent } else { icon_col };
            if i == 0 {
                rects.push(Rect {
                    rect: RectPct::new(0.0, y - 0.02, 0.15, 0.10),
                    color: accent_soft,
                });
            }
            pills.push(Pill {
                rect: RectPct::new(0.02, y, 0.018, 0.024),
                color: dot,
            });
            pills.push(Pill {
                rect: RectPct::new(0.05, y - 0.002, 0.075, 0.026),
                color: bar,
            });
            pills.push(Pill {
                rect: RectPct::new(0.02, y + 0.042, 0.10, 0.02),
                color: bar_dim,
            });
        }
        left_edge = 0.15;
        content_top = 0.04;
    } else {
        // Top bar of tabs; each = status dot + title bar.
        rects.push(Rect {
            rect: RectPct::new(0.0, 0.0, 1.0, 0.09),
            color: panel,
        });
        for i in 0..3 {
            let x = 0.03 + i as f32 * 0.18;
            let dot = if i == 0 { accent } else { icon_col };
            if i == 0 {
                pills.push(Pill {
                    rect: RectPct::new(x - 0.012, 0.02, 0.175, 0.05),
                    color: accent_soft,
                });
            }
            pills.push(Pill {
                rect: RectPct::new(x, 0.033, 0.017, 0.024),
                color: dot,
            });
            pills.push(Pill {
                rect: RectPct::new(x + 0.026, 0.033, 0.12, 0.024),
                color: bar,
            });
        }
        left_edge = 0.0;
        content_top = 0.12;
    }

    // ---------- Tools panel: a file tree ----------
    let mut content_left = left_edge + 0.02;
    if tools_enabled {
        let tx = left_edge + 0.015;
        let tw = 0.23;
        rects.push(Rect {
            rect: RectPct::new(tx, content_top, tw, 0.98 - content_top),
            color: panel,
        });
        // folder / nested-file rows to read clearly as a tree
        let tree: [(u8, Icon); 8] = [
            (0, Icon::FolderClosed),
            (1, Icon::File),
            (1, Icon::File),
            (0, Icon::FolderClosed),
            (1, Icon::File),
            (0, Icon::FolderClosed),
            (1, Icon::File),
            (1, Icon::File),
        ];
        for (i, (indent, ic)) in tree.iter().enumerate() {
            let y = content_top + 0.035 + i as f32 * 0.072;
            let ix = tx + 0.025 + *indent as f32 * 0.035;
            icons.push(IconPct {
                icon: *ic,
                color: icon_col,
                center_x: ix,
                center_y: y,
                width_pct: 0.026,
            });
            let w = (tw * 0.6) - (*indent as f32 * 0.035);
            pills.push(Pill {
                rect: RectPct::new(ix + 0.022, y - 0.013, w, 0.024),
                color: bar_dim,
            });
        }
        content_left = tx + tw + 0.025;
    }

    // ---------- Code review: a right panel of file-diff cards ----------
    let content_right = if code_review { 0.65 } else { 0.97 };
    if code_review {
        let cx = 0.67;
        let cw = 0.31;
        rects.push(Rect {
            rect: RectPct::new(cx, content_top, cw, 0.98 - content_top),
            color: panel,
        });
        // header: "Uncommitted changes" bar + a Commit button
        pills.push(Pill {
            rect: RectPct::new(cx + 0.02, content_top + 0.025, cw * 0.5, 0.026),
            color: bar,
        });
        pills.push(Pill {
            rect: RectPct::new(cx + cw - 0.085, content_top + 0.018, 0.065, 0.036),
            color: accent,
        });
        let mut cy = content_top + 0.085;
        for _card in 0..2 {
            let card_h = 0.31;
            rects.push(Rect {
                rect: RectPct::new(cx + 0.015, cy, cw - 0.03, card_h),
                color: card,
            });
            // card header: file icon + filename + green/red change badges
            icons.push(IconPct {
                icon: Icon::File,
                color: icon_col,
                center_x: cx + 0.04,
                center_y: cy + 0.03,
                width_pct: 0.022,
            });
            pills.push(Pill {
                rect: RectPct::new(cx + 0.062, cy + 0.018, cw * 0.36, 0.024),
                color: bar,
            });
            pills.push(Pill {
                rect: RectPct::new(cx + cw - 0.095, cy + 0.018, 0.028, 0.024),
                color: green,
            });
            pills.push(Pill {
                rect: RectPct::new(cx + cw - 0.06, cy + 0.018, 0.028, 0.024),
                color: red,
            });
            // diff rows: line-number bar + code bar, some +/- highlighted
            let rows = [bar_dim, green, red, bar_dim, green, red];
            let widths = [0.62, 0.72, 0.5, 0.66, 0.58, 0.44];
            for (r, (rc, w)) in rows.iter().zip(widths).enumerate() {
                let ry = cy + 0.07 + r as f32 * 0.038;
                pills.push(Pill {
                    rect: RectPct::new(cx + 0.03, ry, 0.018, 0.02),
                    color: bar_dim,
                });
                pills.push(Pill {
                    rect: RectPct::new(cx + 0.058, ry, (cw - 0.09) * w, 0.02),
                    color: *rc,
                });
            }
            cy += card_h + 0.02;
        }
    }

    // ---------- Main content: a minimal agent conversation ----------
    let mw = (content_right - content_left).max(0.15);
    let mut y = content_top + 0.03;
    // avatar + prompt
    pills.push(Pill {
        rect: RectPct::new(content_left, y, 0.032, 0.045),
        color: accent,
    });
    pills.push(Pill {
        rect: RectPct::new(content_left + 0.048, y + 0.008, mw * 0.78, 0.03),
        color: bar,
    });
    y += 0.08;
    pills.push(Pill {
        rect: RectPct::new(content_left + 0.048, y, mw * 0.42, 0.022),
        color: bar_dim,
    });
    y += 0.055;
    // command blocks (rounded rows with a leading check dot)
    for _i in 0..2 {
        let bh = 0.07;
        rects.push(Rect {
            rect: RectPct::new(content_left, y, mw, bh),
            color: panel,
        });
        pills.push(Pill {
            rect: RectPct::new(content_left + 0.018, y + 0.024, 0.018, 0.022),
            color: green,
        });
        pills.push(Pill {
            rect: RectPct::new(content_left + 0.046, y + 0.024, mw * 0.62, 0.024),
            color: bar_dim,
        });
        y += bh + 0.022;
    }

    Align::new(
        OnboardingVisual::new(window, pills, false)
            .with_rects(rects)
            .with_icons(icons)
            .finish(),
    )
    .finish()
}
