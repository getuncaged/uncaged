use std::default::Default;
use std::fmt;
use std::path::{Path, PathBuf};
use std::{fs::copy, io::Write};

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::color::hex_color::coloru_from_hex_string;
use warp_core::ui::theme::{
    AnsiColors, Details, Fill as ThemeFill, Image as ThemeImage, TerminalColors, VerticalGradient,
    WarpTheme,
};
use warpui::assets::asset_cache::AssetSource;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, EventHandler, Fill, Flex, Icon, MainAxisAlignment,
    MainAxisSize, MouseStateHandle, ParentElement, Radius, Rect, SavePosition, ScrollbarWidth,
    Shrinkable, Text,
};
use warpui::fonts::Weight;
use warpui::platform::Cursor;
use warpui::ui_components::button::{ButtonVariant, TextAndIcon, TextAndIconAlignment};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::slider::SliderStateHandle;
use warpui::ui_components::text_input::TextInput;
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::{Appearance, AppearanceManager};
use crate::editor::{EditorView, Event as EditorEvent};
use crate::themes::theme::{InMemoryThemeOptions, ThemeKind};
use crate::user_config;
use crate::{
    send_telemetry_from_ctx, server::telemetry::TelemetryEvent, themes::theme::CustomTheme,
};

/// The number of editable color slots in the manual editor (background + optional gradient bottom,
/// foreground, accent, cursor, then 8 normal + 8 bright ANSI colors).
const NUM_COLOR_SLOTS: usize = 21;
const SLOT_BG: usize = 0;
const SLOT_BG_BOTTOM: usize = 1;
const SLOT_FG: usize = 2;
const SLOT_ACCENT: usize = 3;
const SLOT_CURSOR: usize = 4;
const SLOT_NORMAL_START: usize = 5;
const SLOT_BRIGHT_START: usize = 13;

/// Labels + starting hex for each color slot, in slot order. Seeds the editor with the Uncaged
/// palette.
const COLOR_SLOTS: [(&str, &str); NUM_COLOR_SLOTS] = [
    ("Background", "#15110c"),
    ("Background (bottom)", "#0c0a08"),
    ("Text", "#e9e1d4"),
    ("Accent", "#ff7a18"),
    ("Cursor", "#ffb23a"),
    ("Black", "#4a443b"),
    ("Red", "#ff6b5e"),
    ("Green", "#8fd46e"),
    ("Yellow", "#ffc24a"),
    ("Blue", "#74b4e0"),
    ("Magenta", "#f090c0"),
    ("Cyan", "#5fc9be"),
    ("White", "#e9e1d4"),
    ("Bright black", "#6e655a"),
    ("Bright red", "#ff8a76"),
    ("Bright green", "#b2e38c"),
    ("Bright yellow", "#ffd46e"),
    ("Bright blue", "#9bcbee"),
    ("Bright magenta", "#f7abd6"),
    ("Bright cyan", "#86ded0"),
    ("Bright white", "#fbf6ec"),
];

const THEMES_REPO_NEW_FILE_URL: &str =
    "https://github.com/getuncaged/uncaged-themes/new/main?filename=themes/community/";

/// Which authoring flow the modal is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeCreatorMode {
    /// Manually edit every color, gradient, background image, and transparency.
    Manual,
    /// Generate a theme from the colors extracted from an image.
    FromImage,
}

const BUTTON_PADDING: f32 = 12.;
const BUTTON_FONT_SIZE: f32 = 14.;
const BUTTON_BORDER_RADIUS: f32 = 4.;
const BORDER_WIDTH: f32 = 1.;

const MODAL_SUBHEADER: &str =
    "Automatically generate a theme based on extracted colors from an image (.png, .jpg).";
const IMAGE_PICKER_BUTTON_PRE_SELECT_TEXT: &str = "Select an image";
const IMAGE_PICKER_BUTTON_SELECTING_TEXT: &str = "Selecting image...";
const IMAGE_PICKER_BUTTON_POST_SELECT_TEXT: &str = "Select a new image";
const CANCEL_BUTTON_TEXT: &str = "Cancel";
const CREATE_BUTTON_TEXT: &str = "Create theme";

#[derive(Default)]
struct MouseStateHandles {
    image_picker_mouse_state: MouseStateHandle,
    cancel_mouse_state: MouseStateHandle,
    create_mouse_state: MouseStateHandle,
}

pub struct ThemeCreatorBody {
    button_mouse_states: MouseStateHandles,
    editor: ViewHandle<EditorView>,
    theme_options: Option<InMemoryThemeOptions>,
    image_state: ThemeCreatorImageState,

    // Manual editor state.
    mode: ThemeCreatorMode,
    /// One hex text-input editor per color slot (see `COLOR_SLOTS`).
    color_editors: Vec<ViewHandle<EditorView>>,
    /// The current parsed value of each color slot (kept in sync as the user types valid hex).
    manual_colors: Vec<ColorU>,
    use_gradient: bool,
    is_light: bool,
    /// Background opacity, 0–100 (drives the background fill's alpha, i.e. transparency).
    bg_opacity: u8,
    bg_image: Option<PathBuf>,
    /// Background image opacity, 0–100.
    bg_image_opacity: u8,
    advanced_expanded: bool,
    mode_tab_states: [MouseStateHandle; 2],
    /// Hover/click states for the gradient, light/dark, and advanced toggles.
    toggle_states: [MouseStateHandle; 3],
    bg_opacity_slider: SliderStateHandle,
    bg_image_opacity_slider: SliderStateHandle,
    share_mouse_state: MouseStateHandle,
    pick_image_mouse_state: MouseStateHandle,
    scroll_state: ClippedScrollStateHandle,
}

#[derive(Debug, Clone)]
pub enum ThemeCreatorBodyAction {
    Create,
    OpenFilePicker,
    HandleImageSelected(PathBuf),
    SetBackgroundColor(usize),
    Cancel,
    FilePickerCancelled,
    // Manual editor actions.
    SetMode(ThemeCreatorMode),
    ToggleGradient,
    ToggleLightDark,
    ToggleAdvanced,
    SetBackgroundOpacity(f32),
    SetBackgroundImageOpacity(f32),
    PickBackgroundImage,
    ShareTheme,
}

pub enum ThemeCreatorBodyEvent {
    Close,
    OpenFilePicker,
    SetCustomTheme { theme: ThemeKind },
    ShowErrorToast { message: String },
}

#[derive(Debug)]
pub enum ThemeCreatorImageState {
    Empty,
    Uploading,
    Uploaded,
}

impl fmt::Display for ThemeCreatorImageState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ThemeCreatorImageState::Empty => write!(f, "{IMAGE_PICKER_BUTTON_PRE_SELECT_TEXT}"),
            ThemeCreatorImageState::Uploading => {
                write!(f, "{IMAGE_PICKER_BUTTON_SELECTING_TEXT}")
            }
            ThemeCreatorImageState::Uploaded => {
                write!(f, "{IMAGE_PICKER_BUTTON_POST_SELECT_TEXT}")
            }
        }
    }
}

impl ThemeCreatorBody {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let editor = Self::editor(ctx);

        let mut color_editors = Vec::with_capacity(NUM_COLOR_SLOTS);
        let mut manual_colors = Vec::with_capacity(NUM_COLOR_SLOTS);
        for &(_label, default_hex) in COLOR_SLOTS.iter() {
            let color = coloru_from_hex_string(default_hex).unwrap_or_else(|_| ColorU::black());
            manual_colors.push(color);

            let color_editor =
                ctx.add_typed_action_view(|ctx| EditorView::new(Default::default(), ctx));
            color_editor.update(ctx, |e, ctx| e.set_buffer_text(default_hex, ctx));
            ctx.subscribe_to_view(&color_editor, move |me, _, event, ctx| {
                if let EditorEvent::Edited(_) = event {
                    me.on_manual_edit(ctx);
                }
            });
            color_editors.push(color_editor);
        }

        Self {
            button_mouse_states: Default::default(),
            editor,
            theme_options: None,
            image_state: ThemeCreatorImageState::Empty,
            mode: ThemeCreatorMode::Manual,
            color_editors,
            manual_colors,
            use_gradient: true,
            is_light: false,
            bg_opacity: 100,
            bg_image: None,
            bg_image_opacity: 40,
            advanced_expanded: false,
            mode_tab_states: Default::default(),
            toggle_states: Default::default(),
            bg_opacity_slider: Default::default(),
            bg_image_opacity_slider: Default::default(),
            share_mouse_state: Default::default(),
            pick_image_mouse_state: Default::default(),
            scroll_state: Default::default(),
        }
    }

    fn editor(ctx: &mut ViewContext<Self>) -> ViewHandle<EditorView> {
        let editor = { ctx.add_typed_action_view(|ctx| EditorView::new(Default::default(), ctx)) };
        ctx.subscribe_to_view(&editor, move |me, _, event, ctx| {
            me.handle_editor_event(event, ctx);
        });

        editor
    }

    pub fn handle_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        if let EditorEvent::Edited(_) = event {
            if self.mode == ThemeCreatorMode::Manual {
                self.on_manual_edit(ctx);
                return;
            }
            if let Some(theme_options) = &mut self.theme_options {
                self.editor.update(ctx, |editor, ctx| {
                    theme_options.set_name(editor.buffer_text(ctx));

                    let theme_kind = ThemeKind::InMemory(theme_options.clone());
                    AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                        appearance_manager.set_transient_theme(theme_kind, ctx);
                    });
                });
            }
        }
        ctx.notify();
    }

    pub fn close(&mut self, ctx: &mut ViewContext<Self>) {
        self.theme_options = None;
        self.image_state = ThemeCreatorImageState::Empty;

        ctx.emit(ThemeCreatorBodyEvent::Close);
    }

    /// Called when the modal is shown: reset to a fresh editor so each "New theme" starts clean,
    /// then kick off the live preview so the terminal immediately reflects the default theme.
    pub fn on_shown(&mut self, ctx: &mut ViewContext<Self>) {
        self.reset_manual_state(ctx);
        self.refresh_manual_preview(ctx);
    }

    /// Resets the manual editor back to its defaults (colors, gradient/appearance toggles,
    /// background image, opacities, name). Called each time the modal opens so a previous
    /// session's edits don't linger.
    fn reset_manual_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.mode = ThemeCreatorMode::Manual;
        self.use_gradient = true;
        self.is_light = false;
        self.bg_opacity = 100;
        self.bg_image = None;
        self.bg_image_opacity = 40;
        self.advanced_expanded = false;
        self.theme_options = None;
        self.image_state = ThemeCreatorImageState::Empty;

        self.editor
            .update(ctx, |editor, ctx| editor.set_buffer_text("", ctx));
        for (i, &(_label, default_hex)) in COLOR_SLOTS.iter().enumerate() {
            self.manual_colors[i] =
                coloru_from_hex_string(default_hex).unwrap_or_else(|_| ColorU::black());
            self.color_editors[i]
                .update(ctx, |editor, ctx| editor.set_buffer_text(default_hex, ctx));
        }
    }

    pub fn cancel(&mut self, ctx: &mut ViewContext<Self>) {
        AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
            appearance_manager.clear_transient_theme(ctx);
        });
        self.close(ctx);
    }

    pub fn open_file_picker(&mut self, ctx: &mut ViewContext<Self>) {
        self.image_state = ThemeCreatorImageState::Uploading;
        ctx.notify();
        ctx.emit(ThemeCreatorBodyEvent::OpenFilePicker);
    }

    pub fn handle_file_picker_cancelled(&mut self, ctx: &mut ViewContext<Self>) {
        self.image_state = if self.theme_options.is_some() {
            ThemeCreatorImageState::Uploaded
        } else {
            ThemeCreatorImageState::Empty
        };
        ctx.notify();
    }

    pub fn create_theme(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(theme_options) = self.theme_options.as_mut() {
            let theme_name = theme_options.name();
            let theme_yaml_file_name = format!("{theme_name}.yaml");
            let original_theme_image_path = theme_options.path();
            let original_theme_image_path_clone = original_theme_image_path.clone();

            let image_extension = original_theme_image_path
                .extension()
                .and_then(|extension| extension.to_str());

            let Some(image_extension) = image_extension else {
                self.send_error_toast(
                    "Failed to process selected image. Please try again with a different image."
                        .to_string(),
                    ctx,
                );
                return;
            };

            let dir = user_config::themes_dir();

            theme_options.set_path(dir.join(format!("{theme_name}.{image_extension}")));
            let mut errored = true;
            ThemeCreatorBody::write_theme(
                &theme_options.theme(),
                dir,
                theme_yaml_file_name,
                Some((
                    original_theme_image_path_clone,
                    theme_name.clone(),
                    image_extension,
                )),
                |path| {
                    send_telemetry_from_ctx!(TelemetryEvent::CreateCustomTheme, ctx);
                    ctx.emit(ThemeCreatorBodyEvent::SetCustomTheme {
                        theme: ThemeKind::Custom(CustomTheme::new(theme_name, path)),
                    });
                    errored = false;
                    self.close(ctx);
                    ctx.notify();
                },
            );
            if errored {
                self.send_error_toast("Something went wrong".to_string(), ctx);
            }
        }
    }

    /// Writes a theme to the filesystem. Calls the success callback if successful.
    /// Note: the image option should be (original_theme_image_path, theme_name, image_extension).
    pub fn write_theme<T>(
        theme: &WarpTheme,
        dir: PathBuf,
        theme_yaml_file_name: String,
        image_option: Option<(PathBuf, String, &str)>,
        success_callback: impl FnOnce(PathBuf) -> T,
    ) -> Option<T> {
        if let Ok(theme_yaml) = serde_yaml::to_string(theme) {
            let path = dir.join(theme_yaml_file_name);
            if let Ok(mut file) = crate::util::file::create_file(&path) {
                if write!(file, "{theme_yaml}").is_ok() {
                    match image_option {
                        Some((image_path, theme_name, image_extension)) => {
                            if copy(
                                image_path.clone(),
                                dir.join(format!("{theme_name}.{image_extension}")),
                            )
                            .is_ok()
                            {
                                return Some((success_callback)(path));
                            }
                        }
                        None => return Some((success_callback)(path)),
                    }
                }
            }
        }
        None
    }

    pub fn set_theme_from_image_path(&mut self, path: PathBuf, ctx: &mut ViewContext<Self>) {
        let file_stem_string = path
            .clone()
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();

        ctx.spawn(
            InMemoryThemeOptions::new(file_stem_string.clone(), path.clone()),
            move |theme_creator_body, theme_options, ctx| {
                match theme_options {
                    Ok(theme_options) => {
                        AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                            appearance_manager.clear_transient_theme(ctx);
                        });

                        theme_creator_body.theme_options = Some(theme_options);
                        theme_creator_body.editor.update(ctx, |editor, ctx| {
                            editor.set_buffer_text(&file_stem_string, ctx);
                        });
                        theme_creator_body.image_state = ThemeCreatorImageState::Uploaded;
                    },
                    Err(e) => {
                        theme_creator_body.send_error_toast(
                            format!("Failed to process selected image due to error: {e}. Please try again with a different image."),
                            ctx,
                        );
                    }
                }
            },
        );
    }

    pub fn set_background_color(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        if let Some(theme_options) = &mut self.theme_options {
            theme_options.set_chosen_bg_color_index(index);

            let theme_kind = ThemeKind::InMemory(theme_options.clone());
            AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                appearance_manager.set_transient_theme(theme_kind, ctx);
            });
        }

        ctx.notify();
    }

    fn send_error_toast(&self, message: String, ctx: &mut ViewContext<Self>) {
        ctx.emit(ThemeCreatorBodyEvent::ShowErrorToast { message });
    }

    // ── Manual editor ──────────────────────────────────────────────────────────

    /// Re-reads every color input and refreshes the live preview from the manually-edited theme.
    fn on_manual_edit(&mut self, ctx: &mut ViewContext<Self>) {
        for i in 0..self.color_editors.len() {
            let text = self.color_editors[i].as_ref(ctx).buffer_text(ctx);
            if let Ok(color) = coloru_from_hex_string(text.trim()) {
                self.manual_colors[i] = color;
            }
        }
        self.refresh_manual_preview(ctx);
    }

    fn refresh_manual_preview(&mut self, ctx: &mut ViewContext<Self>) {
        let theme = self.build_manual_theme(ctx, self.bg_image.as_deref());
        AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
            appearance_manager.set_transient_warp_theme(theme, ctx);
        });
        ctx.notify();
    }

    fn manual_name(&self, ctx: &AppContext) -> String {
        let name = self.editor.as_ref(ctx).buffer_text(ctx);
        let name = name.trim();
        if name.is_empty() {
            "Custom Theme".to_string()
        } else {
            name.to_string()
        }
    }

    /// Applies `opacity` (0–100) to `color` as its alpha channel.
    fn with_opacity(color: ColorU, opacity: u8) -> ColorU {
        let alpha = ((opacity.min(100) as f32 / 100.0) * 255.0).round() as u8;
        ColorU::new(color.r, color.g, color.b, alpha)
    }

    /// Builds a [`WarpTheme`] from the current manual editor state.
    fn build_manual_theme(&self, ctx: &AppContext, bg_image_path: Option<&Path>) -> WarpTheme {
        let c = &self.manual_colors;
        let bg_top = Self::with_opacity(c[SLOT_BG], self.bg_opacity);
        let background = if self.use_gradient {
            let bg_bottom = Self::with_opacity(c[SLOT_BG_BOTTOM], self.bg_opacity);
            ThemeFill::VerticalGradient(VerticalGradient::new(bg_top, bg_bottom))
        } else {
            ThemeFill::Solid(bg_top)
        };

        let normal = AnsiColors::new(
            c[SLOT_NORMAL_START].into(),
            c[SLOT_NORMAL_START + 1].into(),
            c[SLOT_NORMAL_START + 2].into(),
            c[SLOT_NORMAL_START + 3].into(),
            c[SLOT_NORMAL_START + 4].into(),
            c[SLOT_NORMAL_START + 5].into(),
            c[SLOT_NORMAL_START + 6].into(),
            c[SLOT_NORMAL_START + 7].into(),
        );
        let bright = AnsiColors::new(
            c[SLOT_BRIGHT_START].into(),
            c[SLOT_BRIGHT_START + 1].into(),
            c[SLOT_BRIGHT_START + 2].into(),
            c[SLOT_BRIGHT_START + 3].into(),
            c[SLOT_BRIGHT_START + 4].into(),
            c[SLOT_BRIGHT_START + 5].into(),
            c[SLOT_BRIGHT_START + 6].into(),
            c[SLOT_BRIGHT_START + 7].into(),
        );

        let details = if self.is_light {
            Details::Lighter
        } else {
            Details::Darker
        };

        let background_image = bg_image_path.map(|path| ThemeImage {
            source: AssetSource::LocalFile {
                path: path.to_string_lossy().into_owned(),
                content_version: None,
            },
            opacity: self.bg_image_opacity,
        });

        WarpTheme::new(
            background,
            c[SLOT_FG],
            ThemeFill::Solid(c[SLOT_ACCENT]),
            Some(ThemeFill::Solid(c[SLOT_CURSOR])),
            Some(details),
            TerminalColors::new(normal, bright),
            background_image,
            Some(self.manual_name(ctx)),
        )
    }

    fn create_manual_theme(&mut self, ctx: &mut ViewContext<Self>) {
        let name = self.manual_name(ctx);
        // Sanitize the file name (a raw name could contain path separators). The display name is
        // preserved in the theme itself.
        let slug = slugify(&name);
        let file_name = format!("{slug}.yaml");
        let dir = user_config::themes_dir();

        // If a background image is set, copy it next to the theme file so the theme is
        // self-contained, and reference the copied file (not the user's original path).
        let src_image = self.bg_image.clone();
        let ext = src_image
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str());
        let saved_image_path = match (&src_image, ext) {
            (Some(_), Some(ext)) => Some(dir.join(format!("{slug}.{ext}"))),
            _ => None,
        };
        let image_option = match (&src_image, ext) {
            (Some(src), Some(ext)) => Some((src.clone(), slug.clone(), ext)),
            _ => None,
        };
        let theme = self.build_manual_theme(ctx, saved_image_path.as_deref());

        let mut errored = true;
        ThemeCreatorBody::write_theme(&theme, dir, file_name, image_option, |path| {
            send_telemetry_from_ctx!(TelemetryEvent::CreateCustomTheme, ctx);
            ctx.emit(ThemeCreatorBodyEvent::SetCustomTheme {
                theme: ThemeKind::Custom(CustomTheme::new(name.clone(), path)),
            });
            errored = false;
            self.close(ctx);
            ctx.notify();
        });
        if errored {
            self.send_error_toast("Something went wrong saving the theme.".to_string(), ctx);
        }
    }

    /// Serializes the current theme and opens a pre-filled GitHub PR against the community themes
    /// repo. No in-app GitHub auth is needed — GitHub handles the fork + PR in the browser.
    fn share_theme(&mut self, ctx: &mut ViewContext<Self>) {
        let name = self.manual_name(ctx);
        let theme = self.build_manual_theme(ctx, self.bg_image.as_deref());
        let yaml = match serde_yaml::to_string(&theme) {
            Ok(yaml) => yaml,
            Err(e) => {
                self.send_error_toast(format!("Couldn't serialize theme for sharing: {e}"), ctx);
                return;
            }
        };
        let slug = slugify(&name);
        let url = format!(
            "{THEMES_REPO_NEW_FILE_URL}{slug}.yaml&value={}",
            urlencoding::encode(&yaml)
        );
        ctx.open_url(&url);
        if self.bg_image.is_some() {
            self.send_error_toast(
                "Opened a PR draft in your browser. Note: attach your background image to the PR — it isn't included automatically.".to_string(),
                ctx,
            );
        }
    }

    fn set_mode(&mut self, mode: ThemeCreatorMode, ctx: &mut ViewContext<Self>) {
        self.mode = mode;
        match mode {
            ThemeCreatorMode::Manual => self.refresh_manual_preview(ctx),
            ThemeCreatorMode::FromImage => {
                if let Some(theme_options) = &self.theme_options {
                    let theme_kind = ThemeKind::InMemory(theme_options.clone());
                    AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                        appearance_manager.set_transient_theme(theme_kind, ctx);
                    });
                } else {
                    AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                        appearance_manager.clear_transient_theme(ctx);
                    });
                }
            }
        }
        ctx.notify();
    }

    fn toggle_gradient(&mut self, ctx: &mut ViewContext<Self>) {
        self.use_gradient = !self.use_gradient;
        self.refresh_manual_preview(ctx);
    }

    fn toggle_light_dark(&mut self, ctx: &mut ViewContext<Self>) {
        self.is_light = !self.is_light;
        self.refresh_manual_preview(ctx);
    }

    fn toggle_advanced(&mut self, ctx: &mut ViewContext<Self>) {
        self.advanced_expanded = !self.advanced_expanded;
        ctx.notify();
    }

    fn set_background_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        self.bg_opacity = value.round().clamp(0.0, 100.0) as u8;
        self.refresh_manual_preview(ctx);
    }

    fn set_background_image_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        self.bg_image_opacity = value.round().clamp(0.0, 100.0) as u8;
        self.refresh_manual_preview(ctx);
    }
}

impl ThemeCreatorBody {
    /// The "Custom | From image" mode switcher shown at the top of the modal.
    fn mode_tabs(&self, appearance: &Appearance) -> Box<dyn Element> {
        Container::new(
            Flex::row()
                .with_child(
                    Container::new(self.pill_button(
                        "Custom",
                        self.mode == ThemeCreatorMode::Manual,
                        self.mode_tab_states[0].clone(),
                        ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::Manual),
                        appearance,
                    ))
                    .with_margin_right(8.)
                    .finish(),
                )
                .with_child(self.pill_button(
                    "From image",
                    self.mode == ThemeCreatorMode::FromImage,
                    self.mode_tab_states[1].clone(),
                    ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                    appearance,
                ))
                .finish(),
        )
        .with_margin_bottom(12.)
        .finish()
    }

    /// A single "label + swatch + hex input" editing row for color slot `index`.
    fn color_field_row(&self, index: usize, appearance: &Appearance) -> Box<dyn Element> {
        let (label, _) = COLOR_SLOTS[index];
        let color = self.manual_colors[index];
        let theme = appearance.theme();

        let swatch = ConstrainedBox::new(
            Rect::new()
                .with_background_color(color)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .with_border(
                    Border::all(1.).with_border_fill(theme.main_text_color(theme.background())),
                )
                .finish(),
        )
        .with_width(26.)
        .with_height(26.)
        .finish();

        let input = ConstrainedBox::new(
            TextInput::new(
                self.color_editors[index].clone(),
                UiComponentStyles::default()
                    .set_border_color(theme.outline().into())
                    .set_font_family_id(appearance.ui_font_family())
                    .set_font_size(13.)
                    .set_background(Fill::None)
                    .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                    .set_padding(Coords::uniform(8.))
                    .set_border_width(1.),
            )
            .build()
            .finish(),
        )
        .with_width(120.)
        .finish();

        let label_el = ConstrainedBox::new(label_text(label, appearance))
            .with_width(130.)
            .finish();

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(label_el)
                .with_child(Container::new(swatch).with_margin_right(10.).finish())
                .with_child(input)
                .finish(),
        )
        .with_vertical_padding(4.)
        .finish()
    }

    /// A small pill button that dispatches `action` when clicked.
    fn pill_button(
        &self,
        label: &str,
        active: bool,
        mouse_state: MouseStateHandle,
        action: ThemeCreatorBodyAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let variant = if active {
            ButtonVariant::Accent
        } else {
            ButtonVariant::Secondary
        };
        appearance
            .ui_builder()
            .button(variant, mouse_state)
            .with_style(UiComponentStyles {
                font_size: Some(13.),
                font_weight: Some(Weight::Bold),
                padding: Some(Coords::uniform(10.)),
                ..Default::default()
            })
            .with_centered_text_label(label.into())
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
            .finish()
    }

    fn opacity_slider(
        &self,
        state: SliderStateHandle,
        value: u8,
        make_action: fn(f32) -> ThemeCreatorBodyAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        appearance
            .ui_builder()
            .slider(state)
            .with_range(0.0..100.0)
            .with_default_value(value as f32)
            .with_style(UiComponentStyles {
                width: Some(180.),
                margin: Some(Coords::default().top(3.).bottom(3.)),
                ..Default::default()
            })
            .on_drag(move |ctx, _, val| ctx.dispatch_typed_action(make_action(val)))
            .on_change(move |ctx, _, val| ctx.dispatch_typed_action(make_action(val)))
            .build()
            .finish()
    }

    fn render_manual(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let tabs = self.mode_tabs(appearance);

        // Name.
        let name_field = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Container::new(label_text("Theme name", appearance)).finish())
            .with_child(
                Container::new(
                    TextInput::new(
                        self.editor.clone(),
                        UiComponentStyles::default()
                            .set_border_color(theme.outline().into())
                            .set_font_family_id(appearance.ui_font_family())
                            .set_font_size(14.)
                            .set_background(Fill::None)
                            .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                            .set_padding(Coords::uniform(10.))
                            .set_border_width(1.),
                    )
                    .build()
                    .finish(),
                )
                .with_margin_top(6.)
                .finish(),
            )
            .finish();

        // Simple section.
        let mut content = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        content.add_child(name_field);
        content.add_child(
            Container::new(section_label("Colors", appearance))
                .with_margin_top(18.)
                .finish(),
        );
        content.add_child(self.color_field_row(SLOT_BG, appearance));
        content.add_child(
            Container::new(self.pill_button(
                if self.use_gradient {
                    "Background gradient: On"
                } else {
                    "Background gradient: Off"
                },
                self.use_gradient,
                self.toggle_states[0].clone(),
                ThemeCreatorBodyAction::ToggleGradient,
                appearance,
            ))
            .with_vertical_padding(4.)
            .finish(),
        );
        if self.use_gradient {
            content.add_child(self.color_field_row(SLOT_BG_BOTTOM, appearance));
        }
        content.add_child(self.color_field_row(SLOT_FG, appearance));
        content.add_child(self.color_field_row(SLOT_ACCENT, appearance));
        content.add_child(self.color_field_row(SLOT_CURSOR, appearance));

        content.add_child(
            Container::new(self.pill_button(
                if self.is_light {
                    "Appearance: Light"
                } else {
                    "Appearance: Dark"
                },
                false,
                self.toggle_states[1].clone(),
                ThemeCreatorBodyAction::ToggleLightDark,
                appearance,
            ))
            .with_vertical_padding(4.)
            .finish(),
        );

        // Background transparency.
        content.add_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        ConstrainedBox::new(label_text("Background opacity", appearance))
                            .with_width(150.)
                            .finish(),
                    )
                    .with_child(self.opacity_slider(
                        self.bg_opacity_slider.clone(),
                        self.bg_opacity,
                        ThemeCreatorBodyAction::SetBackgroundOpacity,
                        appearance,
                    ))
                    .finish(),
            )
            .with_vertical_padding(6.)
            .finish(),
        );

        // Advanced section.
        content.add_child(
            Container::new(self.pill_button(
                if self.advanced_expanded {
                    "▾ Advanced"
                } else {
                    "▸ Advanced"
                },
                false,
                self.toggle_states[2].clone(),
                ThemeCreatorBodyAction::ToggleAdvanced,
                appearance,
            ))
            .with_margin_top(14.)
            .finish(),
        );

        if self.advanced_expanded {
            content.add_child(
                Container::new(section_label("Terminal colors", appearance))
                    .with_margin_top(10.)
                    .finish(),
            );
            for i in SLOT_NORMAL_START..SLOT_BRIGHT_START + 8 {
                content.add_child(self.color_field_row(i, appearance));
            }

            content.add_child(
                Container::new(section_label("Background image", appearance))
                    .with_margin_top(14.)
                    .finish(),
            );
            content.add_child(
                Container::new(self.pill_button(
                    if self.bg_image.is_some() {
                        "Change background image"
                    } else {
                        "Add background image"
                    },
                    false,
                    self.pick_image_mouse_state.clone(),
                    ThemeCreatorBodyAction::PickBackgroundImage,
                    appearance,
                ))
                .with_vertical_padding(4.)
                .finish(),
            );
            if self.bg_image.is_some() {
                content.add_child(
                    Container::new(
                        Flex::row()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_child(
                                ConstrainedBox::new(label_text("Image opacity", appearance))
                                    .with_width(150.)
                                    .finish(),
                            )
                            .with_child(self.opacity_slider(
                                self.bg_image_opacity_slider.clone(),
                                self.bg_image_opacity,
                                ThemeCreatorBodyAction::SetBackgroundImageOpacity,
                                appearance,
                            ))
                            .finish(),
                    )
                    .with_vertical_padding(6.)
                    .finish(),
                );
            }
        }

        let scrollable = ConstrainedBox::new(
            ClippedScrollable::vertical(
                self.scroll_state.clone(),
                Container::new(content.finish()).finish(),
                ScrollbarWidth::Auto,
                theme.disabled_text_color(theme.background()).into(),
                theme.main_text_color(theme.background()).into(),
                Fill::None,
            )
            .finish(),
        )
        .with_height(360.)
        .finish();

        // Action buttons.
        let cancel_button = self.pill_button(
            "Cancel",
            false,
            self.button_mouse_states.cancel_mouse_state.clone(),
            ThemeCreatorBodyAction::Cancel,
            appearance,
        );
        let share_button = self.pill_button(
            "Share…",
            false,
            self.share_mouse_state.clone(),
            ThemeCreatorBodyAction::ShareTheme,
            appearance,
        );
        let create_button = self.pill_button(
            "Save theme",
            true,
            self.button_mouse_states.create_mouse_state.clone(),
            ThemeCreatorBodyAction::Create,
            appearance,
        );

        let buttons = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_child(Shrinkable::new(0.34, cancel_button).finish())
                .with_child(
                    Container::new(Shrinkable::new(0.33, share_button).finish())
                        .with_margin_left(8.)
                        .finish(),
                )
                .with_child(
                    Container::new(Shrinkable::new(0.33, create_button).finish())
                        .with_margin_left(8.)
                        .finish(),
                )
                .finish(),
        )
        .with_margin_top(16.)
        .finish();

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(tabs)
            .with_child(scrollable)
            .with_child(buttons)
            .finish()
    }
}

/// A section heading label.
fn section_label(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Text::new_inline(text.to_string(), appearance.ui_font_family(), 14.)
        .with_color(appearance.theme().active_ui_text_color().into())
        .finish()
}

/// A field label.
fn label_text(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Text::new_inline(text.to_string(), appearance.ui_font_family(), 13.)
        .with_color(appearance.theme().nonactive_ui_text_color().into())
        .finish()
}

/// Turns a theme name into a kebab-case file slug for the shared PR filename.
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "custom-theme".to_string()
    } else {
        slug
    }
}

impl Entity for ThemeCreatorBody {
    type Event = ThemeCreatorBodyEvent;
}

impl View for ThemeCreatorBody {
    fn ui_name() -> &'static str {
        "ThemeCreatorBody"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        if self.mode == ThemeCreatorMode::Manual {
            return self.render_manual(app);
        }

        let appearance = Appearance::as_ref(app);

        let default_button_styles = UiComponentStyles {
            font_size: Some(BUTTON_FONT_SIZE),
            font_family_id: Some(appearance.ui_font_family()),
            font_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().background())
                    .into(),
            ),
            font_weight: Some(Weight::Bold),
            border_radius: Some(CornerRadius::with_all(Radius::Pixels(BUTTON_BORDER_RADIUS))),
            border_color: Some(appearance.theme().outline().into()),
            border_width: Some(BORDER_WIDTH),
            padding: Some(Coords::uniform(BUTTON_PADDING)),
            background: Some(appearance.theme().surface_1().into()),
            ..Default::default()
        };

        let cancel_hovered_styles = UiComponentStyles {
            background: Some(appearance.theme().outline().into()),
            border_color: Some(appearance.theme().accent().into()),
            ..default_button_styles
        };

        let disabled_styles = UiComponentStyles {
            background: Some(appearance.theme().surface_3().into()),
            font_color: Some(appearance.theme().disabled_ui_text_color().into()),
            ..default_button_styles
        };

        let create_default_styles = UiComponentStyles {
            background: Some(appearance.theme().accent().into()),
            border_color: Some(appearance.theme().accent().into()),
            font_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().accent())
                    .into(),
            ),
            ..default_button_styles
        };

        let create_hovered_styles = UiComponentStyles {
            border_color: Some(
                appearance
                    .theme()
                    .main_text_color(appearance.theme().background())
                    .into(),
            ),
            ..create_default_styles
        };

        let image_picker_button_background = if self.theme_options.is_some() {
            appearance.theme().surface_1()
        } else {
            appearance.theme().accent()
        };

        let image_picker_button_hovered_styles = if self.theme_options.is_some() {
            cancel_hovered_styles
        } else {
            UiComponentStyles {
                border_color: Some(appearance.theme().foreground().into()),
                background: Some(image_picker_button_background.into()),
                font_color: Some(
                    appearance
                        .theme()
                        .main_text_color(image_picker_button_background)
                        .into(),
                ),
                ..default_button_styles
            }
        };

        let image_picker_button = appearance.ui_builder().button_with_custom_styles(
            ButtonVariant::Accent,
            self.button_mouse_states.image_picker_mouse_state.clone(),
            UiComponentStyles {
                background: Some(image_picker_button_background.into()),
                font_color: Some(
                    appearance
                        .theme()
                        .main_text_color(image_picker_button_background)
                        .into(),
                ),
                ..default_button_styles
            },
            Some(image_picker_button_hovered_styles),
            Some(image_picker_button_hovered_styles),
            Some(disabled_styles),
        );

        let cancel_button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                self.button_mouse_states.cancel_mouse_state.clone(),
            )
            .with_style(UiComponentStyles {
                font_size: Some(BUTTON_FONT_SIZE),
                font_weight: Some(Weight::Bold),
                padding: Some(Coords::uniform(BUTTON_PADDING)),
                ..Default::default()
            })
            .with_centered_text_label(CANCEL_BUTTON_TEXT.into());

        let mut create_button = appearance
            .ui_builder()
            .button_with_custom_styles(
                ButtonVariant::Basic,
                self.button_mouse_states.create_mouse_state.clone(),
                create_default_styles,
                Some(create_hovered_styles),
                Some(create_hovered_styles),
                Some(disabled_styles),
            )
            .with_centered_text_label(CREATE_BUTTON_TEXT.into());

        let mut flex: Flex = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                Container::new(
                    Text::new_inline(MODAL_SUBHEADER, appearance.ui_font_family(), 14.)
                        .with_color(appearance.theme().active_ui_text_color().into())
                        .finish(),
                )
                .finish(),
            );

        if let Some(theme_options) = &self.theme_options {
            flex.add_child(
                Container::new(
                    Text::new_inline("Theme name", appearance.ui_font_family(), 14.)
                        .with_color(appearance.theme().active_ui_text_color().into())
                        .finish(),
                )
                .with_margin_top(12.)
                .finish(),
            );
            flex.add_child(
                Container::new(
                    TextInput::new(
                        self.editor.clone(),
                        UiComponentStyles::default()
                            .set_border_color(appearance.theme().outline().into())
                            .set_font_family_id(appearance.header_font_family())
                            .set_font_size(14.)
                            .set_background(Fill::None)
                            .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                            .set_padding(Coords::uniform(20.).top(10.).bottom(10.))
                            .set_border_width(2.),
                    )
                    .build()
                    .finish(),
                )
                .with_margin_top(8.)
                .finish(),
            );

            flex.add_child(
                Container::new(
                    Text::new_inline("Background color", appearance.ui_font_family(), 14.)
                        .with_color(appearance.theme().active_ui_text_color().into())
                        .finish(),
                )
                .with_margin_top(24.)
                .finish(),
            );

            let mut color_row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);

            for (bg_color_index, bg_color) in
                theme_options.possible_bg_colors().into_iter().enumerate()
            {
                // Add corner radius if the rect is the first or last one
                let corner_radius = if bg_color_index == 0 {
                    CornerRadius::with_left(Radius::Pixels(8.))
                } else if bg_color_index == 4 {
                    CornerRadius::with_right(Radius::Pixels(8.))
                } else {
                    CornerRadius::with_all(Radius::Pixels(0.))
                };

                // Add a border around the chosen background color
                let border_width = if bg_color_index == theme_options.chosen_bg_color_index() {
                    3.
                } else {
                    0.
                };

                color_row.add_child(
                    Flex::row()
                        .with_child(
                            EventHandler::new(
                                ConstrainedBox::new(
                                    Rect::new()
                                        .with_background_color(bg_color)
                                        .with_corner_radius(corner_radius)
                                        .with_border(
                                            Border::all(border_width).with_border_fill(
                                                appearance.theme().main_text_color(
                                                    appearance.theme().background(),
                                                ),
                                            ),
                                        )
                                        .finish(),
                                )
                                .with_width(110.)
                                .with_height(40.)
                                .finish(),
                            )
                            .on_left_mouse_down(move |ctx, _, _| {
                                ctx.dispatch_typed_action(
                                    ThemeCreatorBodyAction::SetBackgroundColor(bg_color_index),
                                );
                                DispatchEventResult::StopPropagation
                            })
                            .finish(),
                        )
                        .finish(),
                );
            }

            flex.add_child(
                Container::new(color_row.finish())
                    .with_margin_top(8.)
                    .finish(),
            );
        } else {
            create_button = create_button.disabled();
        }

        flex.add_child(
            Container::new(
                if let ThemeCreatorImageState::Uploading = self.image_state {
                    image_picker_button
                        .with_centered_text_label(self.image_state.to_string())
                        .disabled()
                        .build()
                        .finish()
                } else {
                    image_picker_button
                        .with_text_and_icon_label(
                            TextAndIcon::new(
                                TextAndIconAlignment::TextFirst,
                                self.image_state.to_string(),
                                Icon::new("bundled/svg/upload-01.svg", ColorU::white()),
                                MainAxisSize::Max,
                                MainAxisAlignment::Center,
                                vec2f(16., 16.),
                            )
                            .with_inner_padding(4.),
                        )
                        .build()
                        .with_cursor(Cursor::PointingHand)
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(ThemeCreatorBodyAction::OpenFilePicker)
                        })
                        .finish()
                },
            )
            .with_margin_top(24.)
            .finish(),
        );

        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.mode_tabs(appearance))
            .with_child(
                Container::new(
                    flex.with_child(
                        Container::new(
                            Flex::row()
                                .with_child(
                                    Shrinkable::new(
                                        0.5,
                                        Container::new(
                                            SavePosition::new(
                                                cancel_button
                                                    .build()
                                                    .with_cursor(Cursor::PointingHand)
                                                    .on_click(move |ctx, _, _| {
                                                        ctx.dispatch_typed_action(
                                                            ThemeCreatorBodyAction::Cancel,
                                                        )
                                                    })
                                                    .finish(),
                                                "theme_creator_cancel_button",
                                            )
                                            .finish(),
                                        )
                                        .with_margin_right(8.)
                                        .finish(),
                                    )
                                    .finish(),
                                )
                                .with_child(
                                    Shrinkable::new(
                                        0.5,
                                        create_button
                                            .build()
                                            .with_cursor(Cursor::PointingHand)
                                            .on_click(move |ctx, _, _| {
                                                ctx.dispatch_typed_action(
                                                    ThemeCreatorBodyAction::Create,
                                                )
                                            })
                                            .finish(),
                                    )
                                    .finish(),
                                )
                                .with_main_axis_size(MainAxisSize::Max)
                                .finish(),
                        )
                        .with_margin_top(24.)
                        .finish(),
                    )
                    .finish(),
                )
                .finish(),
            )
            .finish()
    }
}

impl TypedActionView for ThemeCreatorBody {
    type Action = ThemeCreatorBodyAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ThemeCreatorBodyAction::Cancel => self.cancel(ctx),
            ThemeCreatorBodyAction::OpenFilePicker => self.open_file_picker(ctx),
            ThemeCreatorBodyAction::SetBackgroundColor(index) => {
                self.set_background_color(*index, ctx)
            }
            ThemeCreatorBodyAction::Create => {
                if self.mode == ThemeCreatorMode::Manual {
                    self.create_manual_theme(ctx);
                } else {
                    self.create_theme(ctx);
                }
            }
            ThemeCreatorBodyAction::HandleImageSelected(path) => {
                if self.mode == ThemeCreatorMode::Manual {
                    self.bg_image = Some(path.clone());
                    self.refresh_manual_preview(ctx);
                } else {
                    self.set_theme_from_image_path(path.clone(), ctx);
                    ctx.notify();
                }
            }
            ThemeCreatorBodyAction::FilePickerCancelled => self.handle_file_picker_cancelled(ctx),
            ThemeCreatorBodyAction::SetMode(mode) => self.set_mode(*mode, ctx),
            ThemeCreatorBodyAction::ToggleGradient => self.toggle_gradient(ctx),
            ThemeCreatorBodyAction::ToggleLightDark => self.toggle_light_dark(ctx),
            ThemeCreatorBodyAction::ToggleAdvanced => self.toggle_advanced(ctx),
            ThemeCreatorBodyAction::SetBackgroundOpacity(value) => {
                self.set_background_opacity(*value, ctx)
            }
            ThemeCreatorBodyAction::SetBackgroundImageOpacity(value) => {
                self.set_background_image_opacity(*value, ctx)
            }
            ThemeCreatorBodyAction::PickBackgroundImage => {
                ctx.emit(ThemeCreatorBodyEvent::OpenFilePicker);
            }
            ThemeCreatorBodyAction::ShareTheme => self.share_theme(ctx),
        }
    }
}
