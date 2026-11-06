use std::default::Default;
use std::fmt;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use palette::{FromColor, Hsv, Srgb};
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{vec2f, Vector2F};
use settings::Setting as _;
use warp_core::ui::color::hex_color::{coloru_from_hex_string, coloru_to_hex_string};
use warp_core::ui::radius;
use warp_core::ui::theme::{
    AnsiColors, ColorScheme, Details, Fill as ThemeFill, Image as ThemeImage, TerminalColors,
    VerticalGradient, WarpTheme,
};
use warpui::assets::asset_cache::AssetSource;
use warpui::elements::{
    Align, Border, CacheOption, Clipped, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, EventHandler, Expanded, Fill, Flex, Hoverable, Icon,
    Image, MainAxisAlignment, MainAxisSize, MouseStateHandle, ParentElement, Radius, Rect,
    SavePosition, Shrinkable, SizeConstraintCondition, SizeConstraintSwitch, Stack, Text, Wrap,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::ui_components::button::{ButtonVariant, TextAndIcon, TextAndIconAlignment};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::slider::SliderStateHandle;
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::ui_components::text_input::TextInput;
use warpui::{
    AppContext, Element, Entity, EventContext, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use crate::appearance::Appearance;
use crate::editor::{EditOrigin, EditorView, Event as EditorEvent};
use crate::server::telemetry::TelemetryEvent;
use crate::settings_view::settings_page::{
    build_toggle_element, render_settings_description, render_sub_header,
    render_sub_header_with_description, settings_card, settings_description_color,
    CONTENT_FONT_SIZE, HEADER_PADDING, SUBHEADER_FONT_SIZE,
};
use crate::themes::theme::{CustomTheme, InMemoryThemeOptions, ThemeKind};
use crate::themes::theme_background_image;
use crate::window_settings::{BackgroundBlurRadius, BackgroundOpacity, WindowSettings};
use crate::{report_if_error, send_telemetry_from_ctx, user_config};

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
    ("Bottom color", "#0c0a08"),
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

/// Starting opacity for a background image, matching the built-in photo themes.
///
/// Koi, Red Rock and Leafy all ship at 30. It reads low, but the number is the share of the photo
/// that survives — the theme's background is painted over it at `100 - opacity`, and that veil is
/// what keeps text legible. Going higher thins the veil and the terminal turns into a wallpaper
/// you can't read.
const DEFAULT_BG_IMAGE_OPACITY: u8 = 30;

use crate::brand::THEMES_REPO_URL as THEMES_REPO_BASE_URL;
/// Where community themes live in the repo, used to build both the new-file and edit URLs.
const COMMUNITY_THEME_DIR: &str = "themes/community";

/// Which authoring flow the modal is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeCreatorMode {
    /// Manually edit every color, gradient, background image, and transparency.
    Manual,
    /// Generate a theme from the colors extracted from an image.
    FromImage,
}

const IMAGE_PICKER_BUTTON_PRE_SELECT_TEXT: &str = "Choose image…";
const IMAGE_PICKER_BUTTON_SELECTING_TEXT: &str = "Reading image…";
const IMAGE_PICKER_BUTTON_POST_SELECT_TEXT: &str = "Change image…";
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
    /// The background gradient's on/off switch.
    gradient_switch_state: SwitchStateHandle,
    /// The "Dark | Light" segments of the appearance control.
    appearance_segment_states: [MouseStateHandle; 2],
    /// The clickable header row that expands and collapses the Advanced card.
    advanced_header_state: MouseStateHandle,
    /// One per colour slot, for the swatch that opens that slot's picker.
    swatch_mouse_states: Vec<MouseStateHandle>,
    bg_opacity_slider: SliderStateHandle,
    bg_image_opacity_slider: SliderStateHandle,
    window_opacity_slider: SliderStateHandle,
    window_blur_slider: SliderStateHandle,
    /// Which colour slot currently has its picker expanded, if any.
    open_picker: Option<usize>,
    /// The open picker's colour as hue (0–360), saturation and value (both 0–1).
    ///
    /// This is the picker's source of truth rather than the slot's `ColorU`, because HSV loses
    /// information at the edges: every fully-dark colour is black whatever its hue, and every
    /// desaturated one is grey. Re-deriving these from the colour each frame would make the hue
    /// strip jump back to red the moment you dragged into a corner, so the wheel remembers where
    /// you actually are and only the *colour* is derived. Photoshop behaves the same way.
    picker_hsv: (f32, f32, f32),
    /// Whether a drag that began inside the saturation/value square is still in progress, so
    /// pointer drags that merely pass over the square don't repaint the colour.
    picker_dragging: bool,
    share_mouse_state: MouseStateHandle,
    pick_image_mouse_state: MouseStateHandle,
    remove_image_mouse_state: MouseStateHandle,

    // "From image" state, beyond the processed image itself (`theme_options`).
    /// Whether the generated theme keeps the picked image as its background picture.
    image_as_background: bool,
    /// How much of the picked image shows through when it is the background, 0–100.
    image_background_opacity: u8,
    image_background_switch_state: SwitchStateHandle,
    image_background_opacity_slider: SliderStateHandle,
    /// One per background candidate tile in step 2.
    candidate_mouse_states: [MouseStateHandle; MAX_BACKGROUND_CANDIDATES],
    customize_mouse_state: MouseStateHandle,
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
    /// Picks the light (`true`) or dark (`false`) UI details for the theme.
    SetLightAppearance(bool),
    ToggleAdvanced,
    SetBackgroundOpacity(f32),
    SetBackgroundImageOpacity(f32),
    SetWindowOpacity(f32),
    SetWindowBlurRadius(f32),
    /// Expand/collapse the colour picker for a colour slot.
    TogglePicker(usize),
    /// A click or drag inside the saturation/value square, in normalized element coordinates:
    /// `x` runs 0 (grey) → 1 (saturated), `y` runs 0 (bright) → 1 (black).
    PickSaturationValue {
        x: f32,
        y: f32,
        start_drag: bool,
    },
    /// A click or drag on the hue strip, as a normalized 0–1 position down the strip.
    PickHue(f32),
    /// The pointer was released, ending any saturation/value drag.
    EndPickerDrag,
    /// Apply a preset swatch (0xRRGGBB) to the slot whose picker is open.
    SetPresetColor(u32),
    PickBackgroundImage,
    /// Drop the custom theme's background picture.
    RemoveBackgroundImage,
    ShareTheme,
    // "From image" actions.
    /// Keep or drop the picked image as the generated theme's background picture.
    ToggleImageAsBackground,
    SetImageBackgroundOpacity(f32),
    /// Open the generated theme in the custom editor, every colour pre-filled, to fine-tune it.
    CustomizeGeneratedTheme,
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
            color_editor.update(ctx, |e, ctx| e.system_reset_buffer_text(default_hex, ctx));
            ctx.subscribe_to_view(&color_editor, move |me, _, event, ctx| {
                // Initialization events are queued and can arrive after this subscription is
                // registered, even while the editor is hidden. Only user edits are read back: the
                // editor's own resets set `manual_colors` themselves.
                if let EditorEvent::Edited(EditOrigin::UserTyped | EditOrigin::UserInitiated) =
                    event
                {
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
            bg_image_opacity: DEFAULT_BG_IMAGE_OPACITY,
            advanced_expanded: false,
            mode_tab_states: Default::default(),
            gradient_switch_state: Default::default(),
            appearance_segment_states: Default::default(),
            advanced_header_state: Default::default(),
            swatch_mouse_states: (0..NUM_COLOR_SLOTS)
                .map(|_| MouseStateHandle::default())
                .collect(),
            bg_opacity_slider: Default::default(),
            bg_image_opacity_slider: Default::default(),
            window_opacity_slider: Default::default(),
            window_blur_slider: Default::default(),
            open_picker: None,
            picker_hsv: (0., 0., 0.),
            picker_dragging: false,
            share_mouse_state: Default::default(),
            pick_image_mouse_state: Default::default(),
            remove_image_mouse_state: Default::default(),
            image_as_background: true,
            image_background_opacity: DEFAULT_BG_IMAGE_OPACITY,
            image_background_switch_state: Default::default(),
            image_background_opacity_slider: Default::default(),
            candidate_mouse_states: Default::default(),
            customize_mouse_state: Default::default(),
        }
    }

    fn editor(ctx: &mut ViewContext<Self>) -> ViewHandle<EditorView> {
        let editor = { ctx.add_typed_action_view(|ctx| EditorView::new(Default::default(), ctx)) };
        // An empty name saves as "Custom Theme" (see `manual_name`), so say so up front.
        editor.update(ctx, |editor, ctx| {
            editor.set_placeholder_text("Custom Theme", ctx)
        });
        ctx.subscribe_to_view(&editor, move |me, _, event, ctx| {
            me.handle_editor_event(event, ctx);
        });

        editor
    }

    pub fn handle_editor_event(&mut self, event: &EditorEvent, ctx: &mut ViewContext<Self>) {
        // Only the user's own typing counts. The editor also fills this field itself — resetting
        // it, or naming a theme after its image — and those values are already where they belong.
        if let EditorEvent::Edited(origin) = event {
            if origin.is_user() {
                match self.mode {
                    ThemeCreatorMode::Manual => {
                        self.on_manual_edit(ctx);
                        return;
                    }
                    ThemeCreatorMode::FromImage => {
                        let name = self.editor.as_ref(ctx).buffer_text(ctx);
                        if let Some(theme_options) = &mut self.theme_options {
                            theme_options.set_name(name);
                        }
                    }
                }
            }
        }
        ctx.notify();
    }

    pub fn close(&mut self, ctx: &mut ViewContext<Self>) {
        self.theme_options = None;
        self.image_state = ThemeCreatorImageState::Empty;

        ctx.emit(ThemeCreatorBodyEvent::Close);
    }

    /// Called when the modal is shown: reset to a fresh editor so each "New theme" starts clean.
    pub fn on_shown(&mut self, ctx: &mut ViewContext<Self>) {
        self.reset_manual_state(ctx);
        ctx.notify();
    }

    /// Resets the manual editor back to its defaults (colors, gradient/appearance toggles,
    /// background image, opacities, name). Called each time the modal opens so a previous
    /// session's edits don't linger.
    fn reset_manual_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.mode = ThemeCreatorMode::Manual;
        self.reset_editor_state(ctx);
    }

    /// Puts every field back to its default — colours, toggles, opacities, the background image,
    /// the name, and any image being turned into a theme — while staying in the current mode.
    ///
    /// The fields are refilled with system edits, which the editor subscriptions ignore, so a
    /// reset arriving late can't overwrite anything the user does next.
    fn reset_editor_state(&mut self, ctx: &mut ViewContext<Self>) {
        self.use_gradient = true;
        self.is_light = false;
        self.bg_opacity = 100;
        self.bg_image = None;
        self.bg_image_opacity = DEFAULT_BG_IMAGE_OPACITY;
        self.advanced_expanded = false;
        self.open_picker = None;
        self.picker_dragging = false;
        self.theme_options = None;
        self.image_state = ThemeCreatorImageState::Empty;
        self.image_as_background = true;
        self.image_background_opacity = DEFAULT_BG_IMAGE_OPACITY;

        self.editor
            .update(ctx, |editor, ctx| editor.system_reset_buffer_text("", ctx));
        for (i, &(_label, default_hex)) in COLOR_SLOTS.iter().enumerate() {
            self.manual_colors[i] =
                coloru_from_hex_string(default_hex).unwrap_or_else(|_| ColorU::black());
            self.color_editors[i].update(ctx, |editor, ctx| {
                editor.system_reset_buffer_text(default_hex, ctx)
            });
        }
    }

    /// Discards everything in progress, in either mode. On the settings page the editor stays
    /// open, so it starts over from a clean slate rather than keeping the half-made theme — or the
    /// name of the last one — around. The app's theme was never touched, so there is nothing to
    /// put back.
    pub fn cancel(&mut self, ctx: &mut ViewContext<Self>) {
        self.reset_editor_state(ctx);
        self.close(ctx);
        ctx.notify();
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
        let Some(mut theme_options) = self.theme_options.clone() else {
            return;
        };
        let theme_name = self.image_theme_name(&theme_options, ctx);
        theme_options.set_name(theme_name.clone());
        // Sanitize the name before it becomes a filename — a name can contain path separators
        // or `..`. The display name is preserved in the theme itself; only the on-disk paths
        // use the slug. This matches how the manual editor saves.
        let slug = slugify(&theme_name);
        let theme_yaml_file_name = format!("{slug}.yaml");
        let original_theme_image_path = theme_options.path();

        let dir = user_config::themes_dir();

        // The picture only travels with the theme when it is the background. The import
        // re-encodes to JPEG whatever the source was, so the saved name is fixed.
        let image_option = if self.image_as_background {
            theme_options.set_path(dir.join(format!(
                "{slug}.{}",
                theme_background_image::IMPORTED_EXTENSION
            )));
            Some((original_theme_image_path, slug.clone()))
        } else {
            None
        };
        let theme = self.generated_theme(&theme_options);
        let mut errored = true;
        ThemeCreatorBody::write_theme(&theme, dir, theme_yaml_file_name, image_option, |path| {
            send_telemetry_from_ctx!(TelemetryEvent::CreateCustomTheme, ctx);
            ctx.emit(ThemeCreatorBodyEvent::SetCustomTheme {
                theme: ThemeKind::Custom(CustomTheme::new(theme_name, path)),
            });
            errored = false;
            // Start the next theme from a clean slate, still in this mode.
            self.reset_editor_state(ctx);
            self.close(ctx);
            ctx.notify();
        });
        if errored {
            self.send_error_toast("Something went wrong".to_string(), ctx);
        }
    }

    /// The name a theme made from an image is saved under: whatever is in the name field, or
    /// the image's file name when the field is empty.
    fn image_theme_name(&self, theme_options: &InMemoryThemeOptions, app: &AppContext) -> String {
        let typed = self.editor.as_ref(app).buffer_text(app);
        let typed = typed.trim();
        if typed.is_empty() {
            theme_options.name()
        } else {
            typed.to_string()
        }
    }

    /// The theme made from the processed image, with the picked background colour and — if it
    /// is switched on — the image itself as the background picture.
    fn generated_theme(&self, theme_options: &InMemoryThemeOptions) -> WarpTheme {
        theme_options.theme_with_background_image(
            self.image_as_background
                .then_some(self.image_background_opacity),
        )
    }

    /// Writes a theme to the filesystem. Calls the success callback if successful.
    /// Note: the image option should be (original_theme_image_path, theme_name). The image is
    /// normalised on the way in and always lands as `<theme_name>.jpg`.
    pub fn write_theme<T>(
        theme: &WarpTheme,
        dir: PathBuf,
        theme_yaml_file_name: String,
        image_option: Option<(PathBuf, String)>,
        success_callback: impl FnOnce(PathBuf) -> T,
    ) -> Option<T> {
        if let Ok(theme_yaml) = serde_yaml::to_string(theme) {
            let path = dir.join(theme_yaml_file_name);
            if let Ok(mut file) = crate::util::file::create_file(&path) {
                if write!(file, "{theme_yaml}").is_ok() {
                    match image_option {
                        // Normalise rather than copy: an imported wallpaper is capped, flattened
                        // and re-encoded so a theme can't carry a 7.7MP screenshot with an alpha
                        // channel that every preview then pays to decode.
                        Some((image_path, theme_name)) => {
                            if theme_background_image::import(&image_path, &dir, &theme_name)
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
                        theme_creator_body.theme_options = Some(theme_options);
                        theme_creator_body.editor.update(ctx, |editor, ctx| {
                            editor.system_reset_buffer_text(&file_stem_string, ctx);
                        });
                        theme_creator_body.image_state = ThemeCreatorImageState::Uploaded;
                        ctx.notify();
                    },
                    Err(e) => {
                        theme_creator_body.handle_file_picker_cancelled(ctx);
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
        }
        ctx.notify();
    }

    fn toggle_image_as_background(&mut self, ctx: &mut ViewContext<Self>) {
        self.image_as_background = !self.image_as_background;
        ctx.notify();
    }

    fn set_image_background_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        self.image_background_opacity = value.round().clamp(0.0, 100.0) as u8;
        ctx.notify();
    }

    /// Step 3 of "From image": moves the generated theme into the custom editor — name, every
    /// colour including the 16 terminal colours, and the image if it is the background — so it
    /// can be fine-tuned, saved and shared like any custom theme.
    ///
    /// The processed image stays put, so going back to "From image" picks up where it left off.
    fn customize_generated_theme(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(theme_options) = self.theme_options.clone() else {
            return;
        };
        let name = self.image_theme_name(&theme_options, ctx);
        let theme = self.generated_theme(&theme_options);
        let background = theme.background().into_solid();
        let terminal_colors = theme.terminal_colors();
        let normal = &terminal_colors.normal;
        let bright = &terminal_colors.bright;

        let mut colors = vec![
            background,
            // The generated background is a flat colour, so the gradient starts switched off; its
            // bottom colour starts as the same colour in case it is switched on.
            background,
            theme.foreground().into_solid(),
            theme.accent().into_solid(),
            theme.cursor().into_solid(),
        ];
        for ansi in [normal, bright] {
            colors.extend(
                [
                    ansi.black,
                    ansi.red,
                    ansi.green,
                    ansi.yellow,
                    ansi.blue,
                    ansi.magenta,
                    ansi.cyan,
                    ansi.white,
                ]
                .map(ColorU::from),
            );
        }
        debug_assert_eq!(colors.len(), NUM_COLOR_SLOTS);

        for (index, color) in colors.into_iter().enumerate().take(NUM_COLOR_SLOTS) {
            self.manual_colors[index] = color;
            let hex = coloru_to_hex_string(&color);
            self.color_editors[index].update(ctx, |editor, ctx| {
                editor.system_reset_buffer_text(&hex, ctx)
            });
        }
        self.editor.update(ctx, |editor, ctx| {
            editor.system_reset_buffer_text(&name, ctx)
        });

        self.use_gradient = false;
        self.is_light = matches!(theme.inferred_color_scheme(), ColorScheme::DarkOnLight);
        self.bg_opacity = 100;
        self.bg_image = self.image_as_background.then(|| theme_options.path());
        self.bg_image_opacity = self.image_background_opacity;
        // The generated terminal colours live under Advanced, so open it to show them.
        self.advanced_expanded = true;
        self.open_picker = None;
        self.mode = ThemeCreatorMode::Manual;
        ctx.notify();
    }

    fn send_error_toast(&self, message: String, ctx: &mut ViewContext<Self>) {
        ctx.emit(ThemeCreatorBodyEvent::ShowErrorToast { message });
    }

    // ── Manual editor ──────────────────────────────────────────────────────────

    /// Re-reads every color input so the preview card shows the manually-edited theme.
    fn on_manual_edit(&mut self, ctx: &mut ViewContext<Self>) {
        for i in 0..self.color_editors.len() {
            let text = self.color_editors[i].as_ref(ctx).buffer_text(ctx);
            if let Ok(color) = coloru_from_hex_string(text.trim()) {
                self.manual_colors[i] = color;
            }
        }
        // Typing a hex is the other half of the same control, so move the wheel to match it.
        if let Some(index) = self.open_picker {
            self.resync_picker_from_slot(index);
        }
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
        // With a background image the fill stops being a colour and becomes the *veil* over the
        // photo, and the app already works out how opaque that veil should be: it multiplies the
        // fill's alpha by `(100 - image.opacity)` and again by the window opacity. Baking a third
        // opacity in here compounds with both — at a 75% window and a 40% image, a "60%"
        // background lands at 27% and the photo swamps the text. Every built-in image theme
        // (Koi, Red Rock, Leafy) keeps its fill fully opaque for exactly this reason, so match
        // them and let image + window opacity be the only two knobs that matter.
        let bg_alpha = if bg_image_path.is_some() {
            100
        } else {
            self.bg_opacity
        };
        let bg_top = Self::with_opacity(c[SLOT_BG], bg_alpha);
        let background = if self.use_gradient {
            let bg_bottom = Self::with_opacity(c[SLOT_BG_BOTTOM], bg_alpha);
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
        // The import always re-encodes to JPEG, so the saved name is known ahead of time and no
        // longer depends on what the user picked.
        let src_image = self.bg_image.clone();
        let saved_image_path = src_image.as_ref().map(|_| {
            dir.join(format!(
                "{slug}.{}",
                theme_background_image::IMPORTED_EXTENSION
            ))
        });
        let image_option = src_image.as_ref().map(|src| (src.clone(), slug.clone()));
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
    ///
    /// Sharing the *same* theme twice used to open a second "add this theme" PR competing with the
    /// first, because the URL always pointed at GitHub's new-file editor. Instead, look upstream
    /// first and edit whatever is already there — see [`ShareTarget`].
    fn share_theme(&mut self, ctx: &mut ViewContext<Self>) {
        let name = self.manual_name(ctx);
        let slug = slugify(&name);

        // Share the image by the name it will have *in the gallery repo*, not the path it happens
        // to sit at on this machine. `self.bg_image` is wherever the user picked the file from, so
        // serializing it verbatim produced a YAML pointing at someone's Pictures folder. A
        // relative name works because the deserializer resolves anything non-absolute against the
        // themes dir, so it lands correctly on whoever installs the theme next.
        let shared_image = self.bg_image.as_ref().map(|_| {
            PathBuf::from(format!(
                "./{slug}.{}",
                theme_background_image::IMPORTED_EXTENSION
            ))
        });
        let theme = self.build_manual_theme(ctx, shared_image.as_deref());
        let yaml = match serde_yaml::to_string(&theme) {
            Ok(yaml) => yaml,
            Err(e) => {
                self.send_error_toast(format!("Couldn't serialize theme for sharing: {e}"), ctx);
                return;
            }
        };
        let path = format!("{COMMUNITY_THEME_DIR}/{slug}.yaml");
        // Named so the toast can tell the user exactly which file to attach and what to call it.
        let image_file_name = shared_image
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned());

        let lookup_path = path.clone();
        ctx.spawn(
            async move { resolve_share_target(&lookup_path).await },
            move |me, target, ctx| {
                let encoded = urlencoding::encode(&yaml);
                let (url, note) = match &target {
                    ShareTarget::ExistingPullRequest {
                        repo,
                        branch,
                        number,
                    } => (
                        format!("https://github.com/{repo}/edit/{branch}/{path}?value={encoded}"),
                        Some(format!(
                            "Updating your open PR #{number} — committing here adds to it instead \
                             of opening a second one."
                        )),
                    ),
                    ShareTarget::MergedOnMain => (
                        format!("{THEMES_REPO_BASE_URL}/edit/main/{path}?value={encoded}"),
                        Some(
                            "This theme is already in the gallery — opening a PR that updates it."
                                .to_string(),
                        ),
                    ),
                    ShareTarget::New => (
                        format!("{THEMES_REPO_BASE_URL}/new/main?filename={path}&value={encoded}"),
                        None,
                    ),
                };
                ctx.open_url(&url);

                // A GitHub file-editor URL carries text only — a background image cannot ride a
                // query string at any size, so attaching it stays a manual step. The theme now
                // references it by the name below, so that is what the file has to be called.
                let image_note = image_file_name.as_ref().map(|file_name| {
                    format!(
                        "Attach your background image to the PR as \"{file_name}\" — it can't be \
                         included automatically."
                    )
                });
                let message = match (note, image_note) {
                    (Some(note), Some(image)) => Some(format!("{note} {image}")),
                    (Some(note), None) => Some(note),
                    (None, Some(image)) => Some(image),
                    (None, None) => None,
                };
                if let Some(message) = message {
                    me.send_error_toast(message, ctx);
                }
            },
        );
    }

    /// Switches between the custom editor and "From image". Only the editor changes: the app's
    /// theme is left alone, and the preview card shows whichever theme the new mode is making.
    fn set_mode(&mut self, mode: ThemeCreatorMode, ctx: &mut ViewContext<Self>) {
        self.mode = mode;
        ctx.notify();
    }

    fn toggle_gradient(&mut self, ctx: &mut ViewContext<Self>) {
        self.use_gradient = !self.use_gradient;
        ctx.notify();
    }

    /// Sets the theme's UI details to light or dark. Clicking the segment that is already
    /// selected changes nothing.
    fn set_light_appearance(&mut self, is_light: bool, ctx: &mut ViewContext<Self>) {
        if self.is_light == is_light {
            return;
        }
        self.is_light = is_light;
        ctx.notify();
    }

    fn toggle_advanced(&mut self, ctx: &mut ViewContext<Self>) {
        self.advanced_expanded = !self.advanced_expanded;
        ctx.notify();
    }

    fn set_background_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        self.bg_opacity = value.round().clamp(0.0, 100.0) as u8;
        ctx.notify();
    }

    fn set_background_image_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        self.bg_image_opacity = value.round().clamp(0.0, 100.0) as u8;
        ctx.notify();
    }

    /// Expands the colour wheel for `index`, collapsing whichever one was open, and seeds the
    /// wheel's position from that slot's current colour.
    fn toggle_picker(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        self.open_picker = if self.open_picker == Some(index) {
            None
        } else {
            self.picker_hsv = coloru_to_hsv(self.manual_colors[index]);
            Some(index)
        };
        self.picker_dragging = false;
        ctx.notify();
    }

    /// Moves the wheel's crosshair to a point in the saturation/value square. `x`/`y` arrive
    /// normalized to the square, with `y` measured downwards from the bright edge.
    fn pick_saturation_value(
        &mut self,
        x: f32,
        y: f32,
        start_drag: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        if start_drag {
            self.picker_dragging = true;
        } else if !self.picker_dragging {
            // A drag that began somewhere else and happens to pass over the square shouldn't
            // repaint the colour under the pointer.
            return;
        }
        self.picker_hsv.1 = x.clamp(0., 1.);
        self.picker_hsv.2 = (1.0 - y).clamp(0., 1.);
        self.apply_picker_hsv(ctx);
    }

    /// Moves the hue strip's marker, keeping saturation and value where the user left them.
    fn pick_hue(&mut self, position: f32, ctx: &mut ViewContext<Self>) {
        self.picker_hsv.0 = (position.clamp(0., 1.) * 360.0).clamp(0., 360.0);
        self.apply_picker_hsv(ctx);
    }

    /// Writes the wheel's current HSV position into the open slot as a concrete colour.
    fn apply_picker_hsv(&mut self, ctx: &mut ViewContext<Self>) {
        let (hue, saturation, value) = self.picker_hsv;
        self.apply_color_to_open_slot(hsv_to_coloru(hue, saturation, value), ctx);
    }

    /// Writes `color` into the slot whose picker is open, keeping the hex field in sync so the two
    /// inputs never disagree, and refreshes the preview card.
    fn apply_color_to_open_slot(&mut self, color: ColorU, ctx: &mut ViewContext<Self>) {
        let Some(index) = self.open_picker else {
            return;
        };
        self.manual_colors[index] = color;
        let hex = coloru_to_hex_string(&color);
        self.color_editors[index].update(ctx, |editor, ctx| {
            editor.set_buffer_text(&hex, ctx);
        });
        ctx.notify();
    }

    /// Re-seeds the wheel from the open slot's colour after that colour was changed by something
    /// other than the wheel — typing in the hex field, or clicking a preset swatch.
    ///
    /// A colour that is fully dark or fully grey doesn't say what hue it came from, so in those
    /// cases the wheel keeps the hue it already had rather than snapping the strip back to red.
    fn resync_picker_from_slot(&mut self, index: usize) {
        let (hue, saturation, value) = coloru_to_hsv(self.manual_colors[index]);
        let hue = if saturation <= f32::EPSILON || value <= f32::EPSILON {
            self.picker_hsv.0
        } else {
            hue
        };
        self.picker_hsv = (hue, saturation, value);
    }

    /// Window opacity and blur are window-level settings rather than theme fields, but they're
    /// part of "how my terminal looks", so they're editable here and applied live to every window.
    fn set_window_opacity(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        let value = value.round() as u8;
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings.background_opacity.set_value(value, ctx));
        });
        ctx.notify();
    }

    fn set_window_blur_radius(&mut self, value: f32, ctx: &mut ViewContext<Self>) {
        let value = value.round() as u8;
        ctx.windows().set_all_windows_background_blur_radius(value);
        WindowSettings::handle(ctx).update(ctx, |window_settings, ctx| {
            report_if_error!(window_settings.background_blur_radius.set_value(value, ctx));
        });
        ctx.notify();
    }
}

impl ThemeCreatorBody {
    /// The theme the preview card shows: the one being edited in the manual editor, the theme
    /// generated from the picked image, or — before any image is picked — the current theme.
    fn preview_theme(&self, app: &AppContext) -> WarpTheme {
        match self.mode {
            ThemeCreatorMode::Manual => self.build_manual_theme(app, self.bg_image.as_deref()),
            ThemeCreatorMode::FromImage => match &self.theme_options {
                Some(theme_options) => self.generated_theme(theme_options),
                None => Appearance::as_ref(app).theme().clone(),
            },
        }
    }

    /// The "Custom | From image" mode switcher shown at the top of the editor.
    fn mode_tabs(&self, appearance: &Appearance) -> Box<dyn Element> {
        Container::new(
            Align::new(segmented_control(
                vec![
                    Segment {
                        label: "Custom",
                        active: self.mode == ThemeCreatorMode::Manual,
                        mouse_state: self.mode_tab_states[0].clone(),
                        action: ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::Manual),
                    },
                    Segment {
                        label: "From image",
                        active: self.mode == ThemeCreatorMode::FromImage,
                        mouse_state: self.mode_tab_states[1].clone(),
                        action: ThemeCreatorBodyAction::SetMode(ThemeCreatorMode::FromImage),
                    },
                ],
                appearance,
            ))
            .left()
            .finish(),
        )
        .with_margin_bottom(CARD_SPACING)
        .finish()
    }

    /// A single colour row for slot `index`: its label on the left, and on the right the swatch
    /// (which opens the picker) beside the hex field. Every colour row puts these two in the same
    /// right-aligned column, so they line up with each other and with the sliders.
    fn color_field_row(&self, index: usize, appearance: &Appearance) -> Box<dyn Element> {
        self.color_field_row_with_caption(index, None, appearance)
    }

    /// A colour row with a muted caption under its label, saying where the colour shows up.
    fn color_field_row_with_caption(
        &self,
        index: usize,
        caption: Option<&str>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let (label, _) = COLOR_SLOTS[index];
        let color = self.manual_colors[index];
        let theme = appearance.theme();

        // The swatch is the picker's affordance: click it to open the colour wheel for this
        // colour. The hex field beside it stays editable for anyone who'd rather type a value.
        let is_open = self.open_picker == Some(index);
        let swatch = Hoverable::new(self.swatch_mouse_states[index].clone(), |_| {
            ConstrainedBox::new(
                Rect::new()
                    .with_background_color(color)
                    .with_corner_radius(radius::small())
                    .with_border(Border::all(if is_open { 2. } else { 1. }).with_border_fill(
                        if is_open {
                            theme.accent()
                        } else {
                            theme.outline()
                        },
                    ))
                    .finish(),
            )
            .with_width(SWATCH_SIZE)
            .with_height(SWATCH_SIZE)
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(ThemeCreatorBodyAction::TogglePicker(index));
        })
        .finish();

        let input = ConstrainedBox::new(
            TextInput::new(
                self.color_editors[index].clone(),
                hex_input_styles(appearance),
            )
            .build()
            .finish(),
        )
        .with_width(HEX_INPUT_WIDTH)
        .finish();

        let control = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Container::new(swatch).with_margin_right(8.).finish())
            .with_child(input)
            .finish();

        let row = build_toggle_element(
            row_label(label, appearance),
            control,
            appearance,
            caption.map(str::to_string),
        );
        if !is_open {
            return row;
        }

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(row);
        column.add_child(self.render_color_picker(color, appearance));
        column.finish()
    }

    /// The expanded colour wheel for a slot: a saturation/value square with a crosshair you drag,
    /// a hue strip down the side, and quick-pick swatches — the arrangement every image editor
    /// uses. The hex field in the row above stays live the whole time, so you can drag to find a
    /// shade or type an exact value, whichever you prefer.
    fn render_color_picker(&self, _color: ColorU, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let (hue, saturation, value) = self.picker_hsv;
        let outline = theme.disabled_text_color(theme.background());

        let wheel = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(self.render_saturation_value_square(hue, saturation, value, outline))
            .with_child(
                Container::new(self.render_hue_strip(hue, outline))
                    .with_margin_left(PICKER_GAP)
                    .finish(),
            )
            .finish();

        let mut picker = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);
        picker.add_child(wheel);
        picker.add_child(
            Container::new(self.render_preset_swatches(outline))
                .with_margin_top(10.)
                .finish(),
        );

        Container::new(picker.finish())
            .with_margin_bottom(HEADER_PADDING)
            .finish()
    }

    /// The saturation/value square.
    ///
    /// The renderer draws two-stop linear gradients, so a genuine 2D field is built the way image
    /// editors build it: the pure hue underneath, a white-to-clear wash across for saturation, and
    /// a clear-to-black wash down for value. Stacked, they multiply out to the familiar square.
    fn render_saturation_value_square(
        &self,
        hue: f32,
        saturation: f32,
        value: f32,
        outline: ThemeFill,
    ) -> Box<dyn Element> {
        let radius = CornerRadius::with_all(Radius::Pixels(6.));

        let mut square = Stack::new();
        square.add_child(
            Rect::new()
                .with_background_color(hsv_to_coloru(hue, 1.0, 1.0))
                .with_corner_radius(radius)
                .finish(),
        );
        square.add_child(
            Rect::new()
                .with_horizontal_background_gradient(OPAQUE_WHITE, CLEAR_WHITE)
                .with_corner_radius(radius)
                .finish(),
        );
        square.add_child(
            Rect::new()
                .with_background_gradient(
                    vec2f(0.0, 0.0),
                    vec2f(0.0, 1.0),
                    CLEAR_BLACK,
                    OPAQUE_BLACK,
                )
                .with_corner_radius(radius)
                .finish(),
        );
        square.add_child(
            Rect::new()
                .with_corner_radius(radius)
                .with_border(Border::all(1.).with_border_fill(outline))
                .finish(),
        );
        square.add_child(render_crosshair(saturation, value));

        EventHandler::new(
            SavePosition::new(
                ConstrainedBox::new(square.finish())
                    .with_width(SV_SQUARE_WIDTH)
                    .with_height(SV_SQUARE_HEIGHT)
                    .finish(),
                SV_SQUARE_POSITION_ID,
            )
            .finish(),
        )
        .on_left_mouse_down(|ctx, _, position| {
            if let Some((x, y)) = normalized_position_in(ctx, SV_SQUARE_POSITION_ID, position) {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::PickSaturationValue {
                    x,
                    y,
                    start_drag: true,
                });
            }
            DispatchEventResult::StopPropagation
        })
        .on_mouse_dragged(|ctx, _, position| {
            if let Some((x, y)) = normalized_position_in(ctx, SV_SQUARE_POSITION_ID, position) {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::PickSaturationValue {
                    x,
                    y,
                    start_drag: false,
                });
            }
            DispatchEventResult::StopPropagation
        })
        .on_left_mouse_up(|ctx, _, _| {
            ctx.dispatch_typed_action(ThemeCreatorBodyAction::EndPickerDrag);
            DispatchEventResult::StopPropagation
        })
        .finish()
    }

    /// The hue strip beside the square: the spectrum as six two-stop gradients stacked end to end,
    /// with a marker showing where the current hue sits.
    fn render_hue_strip(&self, hue: f32, outline: ThemeFill) -> Box<dyn Element> {
        let segment_height = SV_SQUARE_HEIGHT / (HUE_STOPS.len() - 1) as f32;

        let mut spectrum = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for pair in HUE_STOPS.windows(2) {
            let (from, to) = (rgb_to_coloru(pair[0]), rgb_to_coloru(pair[1]));
            spectrum.add_child(
                ConstrainedBox::new(
                    Rect::new()
                        .with_background_gradient(vec2f(0.0, 0.0), vec2f(0.0, 1.0), from, to)
                        .finish(),
                )
                .with_width(HUE_STRIP_WIDTH)
                .with_height(segment_height)
                .finish(),
            );
        }

        let mut strip = Stack::new();
        strip.add_child(spectrum.finish());
        strip.add_child(
            Rect::new()
                .with_border(Border::all(1.).with_border_fill(outline))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
                .finish(),
        );
        strip.add_child(render_hue_marker(hue));

        EventHandler::new(
            SavePosition::new(
                ConstrainedBox::new(strip.finish())
                    .with_width(HUE_STRIP_WIDTH)
                    .with_height(SV_SQUARE_HEIGHT)
                    .finish(),
                HUE_STRIP_POSITION_ID,
            )
            .finish(),
        )
        .on_left_mouse_down(|ctx, _, position| {
            if let Some((_, y)) = normalized_position_in(ctx, HUE_STRIP_POSITION_ID, position) {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::PickHue(y));
            }
            DispatchEventResult::StopPropagation
        })
        .on_mouse_dragged(|ctx, _, position| {
            if let Some((_, y)) = normalized_position_in(ctx, HUE_STRIP_POSITION_ID, position) {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::PickHue(y));
            }
            DispatchEventResult::StopPropagation
        })
        .finish()
    }

    /// Quick-pick swatches, so the common choices stay one click away. They wrap rather than
    /// run off the card when the page is narrow.
    fn render_preset_swatches(&self, outline: ThemeFill) -> Box<dyn Element> {
        let mut presets = Wrap::row()
            .with_spacing(5.)
            .with_run_spacing(5.)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        for rgb in PRESET_COLORS {
            let preset = ColorU::from_u32((rgb << 8) | 0xFF);
            presets.add_child(
                EventHandler::new(
                    ConstrainedBox::new(
                        Rect::new()
                            .with_background_color(preset)
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
                            .with_border(Border::all(1.).with_border_fill(outline))
                            .finish(),
                    )
                    .with_width(18.)
                    .with_height(18.)
                    .finish(),
                )
                .on_left_mouse_down(move |ctx, _, _| {
                    ctx.dispatch_typed_action(ThemeCreatorBodyAction::SetPresetColor(rgb));
                    DispatchEventResult::StopPropagation
                })
                .finish(),
            );
        }
        ConstrainedBox::new(presets.finish())
            .with_max_width(SV_SQUARE_WIDTH + PICKER_GAP + HUE_STRIP_WIDTH)
            .finish()
    }

    /// The theme name field, shared by both modes.
    fn render_name_card(&self, appearance: &Appearance) -> Box<dyn Element> {
        let input = Container::new(
            TextInput::new(self.editor.clone(), name_input_styles(appearance))
                .build()
                .finish(),
        )
        .with_padding_bottom(HEADER_PADDING)
        .finish();
        section_card("Name", None, vec![input], appearance)
    }

    /// The left-hand column of the manual editor: one card per group of settings.
    fn render_manual_controls(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        column.add_child(self.render_name_card(appearance));

        column.add_child(section_card(
            "Colors",
            None,
            vec![
                self.color_field_row(SLOT_BG, appearance),
                self.color_field_row(SLOT_FG, appearance),
                self.color_field_row(SLOT_ACCENT, appearance),
                self.color_field_row(SLOT_CURSOR, appearance),
            ],
            appearance,
        ));

        // Background: the gradient switch, its bottom colour, and transparency.
        let gradient_switch = appearance
            .ui_builder()
            .switch(self.gradient_switch_state.clone())
            .check(self.use_gradient)
            .build()
            .on_click(|ctx, _, _| ctx.dispatch_typed_action(ThemeCreatorBodyAction::ToggleGradient))
            .finish();
        let mut background_rows = vec![build_toggle_element(
            row_label("Gradient", appearance),
            gradient_switch,
            appearance,
            Some("Fade the background from its color at the top to another at the bottom.".into()),
        )];
        if self.use_gradient {
            background_rows.push(self.color_field_row(SLOT_BG_BOTTOM, appearance));
        }
        // Background transparency. Hidden once there's a background image: the fill is forced
        // opaque in that case (see `build_manual_theme`), so leaving the slider on screen would
        // offer a control that silently does nothing. With a picture, the background colour
        // becomes the veil over it, and Image opacity below is the knob that matters.
        if self.bg_image.is_none() {
            background_rows.push(slider_row(
                "Opacity",
                None,
                SliderRow {
                    state: self.bg_opacity_slider.clone(),
                    value: self.bg_opacity,
                    range: 0.0..100.0,
                    value_label: format!("{}%", self.bg_opacity),
                    make_action: ThemeCreatorBodyAction::SetBackgroundOpacity,
                },
                appearance,
            ));
        }
        background_rows.push(self.render_background_image_row(appearance));
        if self.bg_image.is_some() {
            background_rows.push(slider_row(
                "Image opacity",
                Some(
                    "How much of the picture shows through. The background color covers the \
                     rest, which keeps text readable."
                        .into(),
                ),
                SliderRow {
                    state: self.bg_image_opacity_slider.clone(),
                    value: self.bg_image_opacity,
                    range: 0.0..100.0,
                    value_label: format!("{}%", self.bg_image_opacity),
                    make_action: ThemeCreatorBodyAction::SetBackgroundImageOpacity,
                },
                appearance,
            ));
        }
        column.add_child(section_card(
            "Background",
            None,
            background_rows,
            appearance,
        ));

        // Window-level appearance. Not part of the theme file, but it's what people actually mean
        // by "how my terminal looks", so it's editable right here and applies live.
        let window_settings = WindowSettings::as_ref(app);
        let window_opacity = *window_settings.background_opacity.value();
        let window_blur = *window_settings.background_blur_radius.value();
        column.add_child(section_card(
            "Window",
            Some("Applies to every window right away. Not saved with the theme."),
            vec![
                slider_row(
                    "Opacity",
                    None,
                    SliderRow {
                        state: self.window_opacity_slider.clone(),
                        value: window_opacity,
                        range: BackgroundOpacity::MIN as f32..BackgroundOpacity::MAX as f32,
                        value_label: format!("{window_opacity}%"),
                        make_action: ThemeCreatorBodyAction::SetWindowOpacity,
                    },
                    appearance,
                ),
                slider_row(
                    "Blur radius",
                    None,
                    SliderRow {
                        state: self.window_blur_slider.clone(),
                        value: window_blur,
                        range: BackgroundBlurRadius::MIN as f32..BackgroundBlurRadius::MAX as f32,
                        value_label: format!("{window_blur}"),
                        make_action: ThemeCreatorBodyAction::SetWindowBlurRadius,
                    },
                    appearance,
                ),
            ],
            appearance,
        ));

        let appearance_control = segmented_control(
            vec![
                Segment {
                    label: "Dark",
                    active: !self.is_light,
                    mouse_state: self.appearance_segment_states[0].clone(),
                    action: ThemeCreatorBodyAction::SetLightAppearance(false),
                },
                Segment {
                    label: "Light",
                    active: self.is_light,
                    mouse_state: self.appearance_segment_states[1].clone(),
                    action: ThemeCreatorBodyAction::SetLightAppearance(true),
                },
            ],
            appearance,
        );
        column.add_child(section_card(
            "Appearance",
            None,
            vec![build_toggle_element(
                row_label("Style", appearance),
                appearance_control,
                appearance,
                Some(
                    "Tunes the contrast of menus and panels for a dark or light background.".into(),
                ),
            )],
            appearance,
        ));

        column.add_child(self.render_advanced_card(appearance));
        column.finish()
    }

    /// The Background card's picture row: a thumbnail and file name once a picture is set, with
    /// buttons to change or remove it; otherwise a line saying what a picture does and a button to
    /// add one.
    fn render_background_image_row(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut buttons = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        let label: Box<dyn Element> = match &self.bg_image {
            Some(path) => {
                buttons.add_child(
                    Container::new(action_button(
                        "Remove",
                        ButtonVariant::Secondary,
                        self.remove_image_mouse_state.clone(),
                        ThemeCreatorBodyAction::RemoveBackgroundImage,
                        appearance,
                    ))
                    .with_margin_right(8.)
                    .finish(),
                );
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(image_thumbnail(path))
                            .with_margin_right(10.)
                            .finish(),
                    )
                    .with_child(
                        Shrinkable::new(
                            1.,
                            Flex::column()
                                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                                .with_child(row_label("Image", appearance))
                                .with_child(render_settings_description(file_name, appearance))
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish()
            }
            None => Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(row_label("Image", appearance))
                .with_child(render_settings_description(
                    "A picture behind the terminal, under the background color.".into(),
                    appearance,
                ))
                .finish(),
        };
        buttons.add_child(action_button(
            if self.bg_image.is_some() {
                "Change…"
            } else {
                "Add image…"
            },
            ButtonVariant::Secondary,
            self.pick_image_mouse_state.clone(),
            ThemeCreatorBodyAction::PickBackgroundImage,
            appearance,
        ));
        build_toggle_element(label, buttons.finish(), appearance, None)
    }

    /// The Advanced card: a clickable header row with a chevron, and — when expanded — the 16
    /// terminal colours, each normal one captioned with where programs use it.
    fn render_advanced_card(&self, appearance: &Appearance) -> Box<dyn Element> {
        let expanded = self.advanced_expanded;
        let header = Hoverable::new(self.advanced_header_state.clone(), |state| {
            let theme = appearance.theme();
            let chevron_color = if state.is_hovered() {
                theme.active_ui_text_color()
            } else {
                theme.nonactive_ui_text_color()
            };
            let chevron = ConstrainedBox::new(
                Icon::new(
                    if expanded {
                        "bundled/svg/chevron-down.svg"
                    } else {
                        "bundled/svg/chevron-right.svg"
                    },
                    chevron_color.into_solid(),
                )
                .finish(),
            )
            .with_width(14.)
            .with_height(14.)
            .finish();
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Shrinkable::new(
                        1.,
                        render_sub_header_with_description(
                            appearance,
                            "Advanced",
                            "The 16 colors programs use in the terminal: errors, git status, ls, \
                             prompts.",
                        ),
                    )
                    .finish(),
                )
                .with_child(
                    Container::new(chevron)
                        .with_margin_left(12.)
                        .with_padding_bottom(HEADER_PADDING)
                        .finish(),
                )
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(|ctx, _, _| ctx.dispatch_typed_action(ThemeCreatorBodyAction::ToggleAdvanced))
        .finish();

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(header);

        if expanded {
            column.add_child(group_label("Normal colors", appearance));
            for (offset, usage) in ANSI_COLOR_USES.iter().enumerate() {
                column.add_child(self.color_field_row_with_caption(
                    SLOT_NORMAL_START + offset,
                    Some(usage),
                    appearance,
                ));
            }
            column.add_child(group_label("Bright colors", appearance));
            column.add_child(
                Container::new(render_settings_description(
                    "Bold or highlighted versions of the same colors, e.g. bold red errors or \
                     bright green success."
                        .into(),
                    appearance,
                ))
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
            );
            for i in SLOT_BRIGHT_START..SLOT_BRIGHT_START + 8 {
                column.add_child(self.color_field_row(i, appearance));
            }
        }

        settings_card(column.finish(), appearance)
            .with_margin_bottom(CARD_SPACING)
            .finish()
    }

    /// The left-hand column of the "From image" flow: three numbered steps. Choose an image; pick
    /// which of its colours is the background (and whether the picture itself stays behind the
    /// terminal); then name it, or open it in the custom editor to fine-tune every colour.
    ///
    /// Steps 2 and 3 are shown from the start, greyed out with a hint, so the whole flow reads at
    /// a glance before anything is picked.
    fn render_from_image_controls(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        column.add_child(self.render_choose_image_step(appearance));
        column.add_child(self.render_pick_background_step(appearance));
        column.add_child(self.render_fine_tune_step(appearance));
        column.finish()
    }

    /// Step 1: the image, with a thumbnail and its file name once it has been read.
    fn render_choose_image_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let picker_button = {
            let button = appearance
                .ui_builder()
                .button(
                    ButtonVariant::Secondary,
                    self.button_mouse_states.image_picker_mouse_state.clone(),
                )
                .with_style(button_styles());
            if let ThemeCreatorImageState::Uploading = self.image_state {
                button
                    .with_centered_text_label(self.image_state.to_string())
                    .disabled()
                    .build()
                    .finish()
            } else {
                button
                    .with_text_and_icon_label(
                        TextAndIcon::new(
                            TextAndIconAlignment::IconFirst,
                            self.image_state.to_string(),
                            Icon::new(
                                "bundled/svg/upload-01.svg",
                                theme.active_ui_text_color().into_solid(),
                            ),
                            MainAxisSize::Min,
                            MainAxisAlignment::Center,
                            vec2f(14., 14.),
                        )
                        .with_inner_padding(6.),
                    )
                    .build()
                    .with_cursor(Cursor::PointingHand)
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(ThemeCreatorBodyAction::OpenFilePicker)
                    })
                    .finish()
            }
        };

        let image_label: Box<dyn Element> = match &self.theme_options {
            Some(theme_options) => {
                let path = theme_options.path();
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let thumbnail = image_thumbnail(&path);
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(Container::new(thumbnail).with_margin_right(10.).finish())
                    .with_child(Shrinkable::new(1., row_label(&file_name, appearance)).finish())
                    .finish()
            }
            None => Text::new_inline(
                "No image yet (.png or .jpg)".to_string(),
                appearance.ui_font_family(),
                CONTROL_FONT_SIZE,
            )
            .with_color(settings_description_color(appearance))
            .finish(),
        };

        step_card(
            1,
            "Choose an image",
            "Uncaged pulls a background, text, accent and the 16 terminal colors out of the \
             image. You can tweak every color afterwards.",
            true,
            vec![build_toggle_element(
                image_label,
                picker_button,
                appearance,
                None,
            )],
            appearance,
        )
    }

    /// Step 2: the background candidates as tiles, and the switch that keeps the picture itself
    /// behind the terminal.
    fn render_pick_background_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        let Some(theme_options) = &self.theme_options else {
            return step_card(
                2,
                "Pick a background",
                "Choose an image first. Its main colors show up here to pick from.",
                false,
                Vec::new(),
                appearance,
            );
        };
        let theme = appearance.theme();

        let chosen = theme_options.chosen_bg_color_index();
        let mut tiles = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (index, color) in theme_options
            .possible_bg_colors()
            .into_iter()
            .enumerate()
            .take(MAX_BACKGROUND_CANDIDATES)
        {
            let selected = index == chosen;
            let tile = Hoverable::new(self.candidate_mouse_states[index].clone(), |state| {
                // The chosen tile gets a ring in the text colour (which reads on any card) and a
                // check mark in whichever of black or white stands out on the tile itself.
                let border = if selected {
                    Border::all(2.).with_border_fill(theme.main_text_color(theme.surface_1()))
                } else if state.is_hovered() {
                    Border::all(1.).with_border_fill(theme.active_ui_text_color())
                } else {
                    Border::all(1.).with_border_fill(theme.outline())
                };
                let mut tile = Stack::new();
                tile.add_child(
                    Rect::new()
                        .with_background_color(color)
                        .with_corner_radius(radius::small())
                        .with_border(border)
                        .finish(),
                );
                if selected {
                    tile.add_child(
                        Align::new(
                            ConstrainedBox::new(
                                Icon::new("bundled/svg/check.svg", contrasting_mark_color(color))
                                    .finish(),
                            )
                            .with_width(16.)
                            .with_height(16.)
                            .finish(),
                        )
                        .finish(),
                    );
                }
                ConstrainedBox::new(tile.finish())
                    .with_height(CANDIDATE_TILE_HEIGHT)
                    .finish()
            })
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::SetBackgroundColor(index))
            })
            .finish();
            let tile = if index == 0 {
                tile
            } else {
                Container::new(tile).with_margin_left(6.).finish()
            };
            tiles.add_child(Expanded::new(1., tile).finish());
        }

        let mut rows = vec![Container::new(tiles.finish())
            .with_padding_bottom(HEADER_PADDING)
            .finish()];

        let image_switch = appearance
            .ui_builder()
            .switch(self.image_background_switch_state.clone())
            .check(self.image_as_background)
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(ThemeCreatorBodyAction::ToggleImageAsBackground)
            })
            .finish();
        rows.push(build_toggle_element(
            row_label("Use the image as the background", appearance),
            image_switch,
            appearance,
            Some("Shows the picture behind the terminal, under the background color.".into()),
        ));
        if self.image_as_background {
            rows.push(slider_row(
                "Image opacity",
                None,
                SliderRow {
                    state: self.image_background_opacity_slider.clone(),
                    value: self.image_background_opacity,
                    range: 0.0..100.0,
                    value_label: format!("{}%", self.image_background_opacity),
                    make_action: ThemeCreatorBodyAction::SetImageBackgroundOpacity,
                },
                appearance,
            ));
        }

        step_card(
            2,
            "Pick a background",
            "The main colors in your image. The one you pick fills the background; the text and \
             accent are chosen to stand out on it.",
            true,
            rows,
            appearance,
        )
    }

    /// Step 3: the name, and the way into the custom editor for everything else.
    fn render_fine_tune_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        if self.theme_options.is_none() {
            return step_card(
                3,
                "Fine-tune",
                "Once your image is read, name the theme or change any of its colors.",
                false,
                Vec::new(),
                appearance,
            );
        }

        let name_input = ConstrainedBox::new(
            TextInput::new(self.editor.clone(), name_input_styles(appearance))
                .build()
                .finish(),
        )
        .with_width(NAME_INPUT_WIDTH)
        .finish();
        let customize = action_button(
            "Customize colors",
            ButtonVariant::Secondary,
            self.customize_mouse_state.clone(),
            ThemeCreatorBodyAction::CustomizeGeneratedTheme,
            appearance,
        );

        step_card(
            3,
            "Fine-tune",
            "Happy with it? Create the theme. Or open it in the custom editor to change any color.",
            true,
            vec![
                build_toggle_element(row_label("Name", appearance), name_input, appearance, None),
                build_toggle_element(
                    row_label("Every color", appearance),
                    customize,
                    appearance,
                    Some(
                        "Opens the Custom tab with this theme filled in — background, text, \
                         accent, cursor, the 16 terminal colors and the image — ready to edit, \
                         save or share."
                            .into(),
                    ),
                ),
            ],
            appearance,
        )
    }

    /// The preview card: a small terminal drawn in the theme being edited.
    fn render_preview_card(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let preview_theme = self.preview_theme(app);
        let description = match self.mode {
            ThemeCreatorMode::Manual => "Updates as you edit.",
            ThemeCreatorMode::FromImage if self.theme_options.is_some() => {
                "The theme made from your image. Updates as you pick."
            }
            ThemeCreatorMode::FromImage => {
                "Choose an image to see the theme it makes. Until then, this is your current look."
            }
        };

        let rows = vec![
            Container::new(render_terminal_preview(&preview_theme, appearance))
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
        ];
        section_card("Preview", Some(description), rows, appearance)
    }

    /// The controls column for whichever mode is showing.
    fn render_controls(&self, app: &AppContext) -> Box<dyn Element> {
        match self.mode {
            ThemeCreatorMode::Manual => self.render_manual_controls(app),
            ThemeCreatorMode::FromImage => self.render_from_image_controls(Appearance::as_ref(app)),
        }
    }

    /// Controls on the left; on the right, the preview with the actions right under it, so the
    /// two read as one "result" panel and the buttons sit near the top of the page instead of
    /// under a long scroll. When there isn't room for two columns (a narrow window, or the
    /// modal), everything stacks: preview, controls, then the actions at the bottom.
    fn render_columns(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let result_panel = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_preview_card(app))
            .with_child(self.render_footer(appearance))
            .finish();
        let wide = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Expanded::new(1., self.render_controls(app)).finish())
            .with_child(
                Container::new(
                    ConstrainedBox::new(result_panel)
                        .with_width(PREVIEW_COLUMN_WIDTH)
                        .finish(),
                )
                .with_margin_left(COLUMN_GAP)
                .finish(),
            )
            .finish();
        let narrow = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_preview_card(app))
            .with_child(self.render_controls(app))
            .with_child(self.render_footer(appearance))
            .finish();
        SizeConstraintSwitch::new(
            wide,
            vec![(
                SizeConstraintCondition::WidthLessThan(TWO_COLUMN_MIN_WIDTH),
                narrow,
            )],
        )
        .finish()
    }

    /// The right-aligned action row. In the custom editor, sharing with the community is the
    /// highlighted action on the far right, so people are nudged to contribute their themes;
    /// saving sits beside it as a secondary button. If the row is wider than its column, the
    /// buttons wrap onto a second line rather than spill past the preview's edge.
    fn render_footer(&self, appearance: &Appearance) -> Box<dyn Element> {
        let cancel = SavePosition::new(
            action_button(
                CANCEL_BUTTON_TEXT,
                ButtonVariant::Secondary,
                self.button_mouse_states.cancel_mouse_state.clone(),
                ThemeCreatorBodyAction::Cancel,
                appearance,
            ),
            "theme_creator_cancel_button",
        )
        .finish();

        let mut buttons: Vec<Box<dyn Element>> = Vec::new();
        match self.mode {
            ThemeCreatorMode::Manual => {
                buttons.push(cancel);
                buttons.push(action_button(
                    "Save theme",
                    ButtonVariant::Secondary,
                    self.button_mouse_states.create_mouse_state.clone(),
                    ThemeCreatorBodyAction::Create,
                    appearance,
                ));
                buttons.push(action_button(
                    "Share with the community",
                    ButtonVariant::Accent,
                    self.share_mouse_state.clone(),
                    ThemeCreatorBodyAction::ShareTheme,
                    appearance,
                ));
            }
            ThemeCreatorMode::FromImage => {
                buttons.push(cancel);
                // Saving straight from here skips the custom editor for anyone happy with the
                // generated theme as it is.
                let create = appearance
                    .ui_builder()
                    .button(
                        ButtonVariant::Accent,
                        self.button_mouse_states.create_mouse_state.clone(),
                    )
                    .with_style(button_styles())
                    .with_centered_text_label(CREATE_BUTTON_TEXT.into());
                // Nothing to create until an image has been turned into a palette.
                let create = if self.theme_options.is_some() {
                    create
                } else {
                    create.disabled()
                };
                buttons.push(
                    create
                        .build()
                        .with_cursor(Cursor::PointingHand)
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(ThemeCreatorBodyAction::Create)
                        })
                        .finish(),
                );
            }
        }

        Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::End)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_run_spacing(8.)
            .with_children(buttons)
            .finish()
    }
}

/// Where a shared theme should land on GitHub.
///
/// Resolved in this order, because each case wants a different GitHub URL and only the last one
/// should ever create a brand-new file.
enum ShareTarget {
    /// An open PR already carries this exact file. Editing it on that PR's own branch lands the
    /// change as another commit on the existing PR, which is what "I tweaked my theme and shared
    /// it again" should do.
    ExistingPullRequest {
        repo: String,
        branch: String,
        number: u64,
    },
    /// The theme is already merged into the gallery. Editing it on `main` opens a PR that updates
    /// the existing file rather than adding a duplicate.
    MergedOnMain,
    /// Nothing upstream yet — the new-file editor is correct.
    New,
}

/// How long to wait on GitHub before falling back to "this theme is new". Sharing must stay
/// responsive; guessing `New` on a slow or offline network is harmless, since that's the old
/// behaviour and GitHub itself will object if the file turns out to exist.
const SHARE_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);

/// Number of open PRs to examine. Far above what this repo will realistically carry, but bounded
/// so a busy gallery can't turn one click into unlimited requests.
const SHARE_LOOKUP_MAX_PRS: usize = 30;

/// Works out which of the [`ShareTarget`]s applies to `path`, using only public, unauthenticated
/// GitHub endpoints. Any failure resolves to [`ShareTarget::New`].
async fn resolve_share_target(path: &str) -> ShareTarget {
    let client = match reqwest::Client::builder()
        .timeout(SHARE_LOOKUP_TIMEOUT)
        // GitHub rejects API requests without one.
        .user_agent(crate::brand::HTTP_USER_AGENT)
        .build()
    {
        Ok(client) => client,
        Err(_) => return ShareTarget::New,
    };

    if let Some(target) = find_open_pull_request_for(&client, path).await {
        return target;
    }

    // Not in any open PR — is it already merged? `raw.githubusercontent.com` answers with a plain
    // 200/404 and doesn't count against the API rate limit.
    let raw_url = format!("{}/main/{path}", crate::brand::THEMES_RAW_BASE);
    match client.get(&raw_url).send().await {
        Ok(response) if response.status().is_success() => ShareTarget::MergedOnMain,
        _ => ShareTarget::New,
    }
}

/// Scans open PRs for one that already touches `path`.
async fn find_open_pull_request_for(client: &reqwest::Client, path: &str) -> Option<ShareTarget> {
    let pulls: serde_json::Value = client
        .get(format!(
            "{}/pulls?state=open&per_page=100",
            crate::brand::THEMES_API_BASE
        ))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    for pull in pulls.as_array()?.iter().take(SHARE_LOOKUP_MAX_PRS) {
        let number = pull.get("number")?.as_u64()?;
        let files: serde_json::Value = client
            .get(format!(
                "{}/pulls/{number}/files",
                crate::brand::THEMES_API_BASE
            ))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        let touches_path = files.as_array().is_some_and(|files| {
            files
                .iter()
                .any(|file| file.get("filename").and_then(|f| f.as_str()) == Some(path))
        });
        if !touches_path {
            continue;
        }

        // The branch lives in the contributor's fork when they lack write access, and in this repo
        // when they don't, so take the repo the PR itself reports rather than assuming.
        let head = pull.get("head")?;
        return Some(ShareTarget::ExistingPullRequest {
            repo: head.get("repo")?.get("full_name")?.as_str()?.to_string(),
            branch: head.get("ref")?.as_str()?.to_string(),
            number,
        });
    }
    None
}

// ── Colour wheel geometry ────────────────────────────────────────────────────
//
// The square's size is fixed here rather than measured, because the crosshair is positioned with
// plain spacers and has to agree with the element it sits on.
const SV_SQUARE_WIDTH: f32 = 232.;
const SV_SQUARE_HEIGHT: f32 = 148.;
const HUE_STRIP_WIDTH: f32 = 20.;
/// The space between the saturation/value square and the hue strip.
const PICKER_GAP: f32 = 10.;
const CROSSHAIR_SIZE: f32 = 14.;
const HUE_MARKER_HEIGHT: f32 = 6.;

/// Position IDs used to turn a global pointer position into one local to the wheel. Only one
/// slot's wheel is open at a time, so one ID apiece is enough.
const SV_SQUARE_POSITION_ID: &str = "theme_creator_sv_square";
const HUE_STRIP_POSITION_ID: &str = "theme_creator_hue_strip";

const OPAQUE_WHITE: ColorU = ColorU {
    r: 255,
    g: 255,
    b: 255,
    a: 255,
};
const CLEAR_WHITE: ColorU = ColorU {
    r: 255,
    g: 255,
    b: 255,
    a: 0,
};
const OPAQUE_BLACK: ColorU = ColorU {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};
const CLEAR_BLACK: ColorU = ColorU {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};

/// The corners of the hue spectrum, red round to red, as gradient stops for the strip.
const HUE_STOPS: [(u8, u8, u8); 7] = [
    (255, 0, 0),
    (255, 255, 0),
    (0, 255, 0),
    (0, 255, 255),
    (0, 0, 255),
    (255, 0, 255),
    (255, 0, 0),
];

fn rgb_to_coloru((r, g, b): (u8, u8, u8)) -> ColorU {
    ColorU::new(r, g, b, 255)
}

/// A transparent box that pushes an overlay to where it belongs inside the wheel.
fn spacer(width: f32, height: f32) -> Box<dyn Element> {
    ConstrainedBox::new(Rect::new().finish())
        .with_width(width)
        .with_height(height)
        .finish()
}

/// The crosshair marking the picked saturation/value.
///
/// It's ringed in white inside and dark outside so it stays visible over any part of the square —
/// a plain white ring vanishes against the top-left corner, and a dark one against the bottom.
fn render_crosshair(saturation: f32, value: f32) -> Box<dyn Element> {
    let half = CROSSHAIR_SIZE / 2.;
    let x = (saturation.clamp(0., 1.) * SV_SQUARE_WIDTH - half)
        .clamp(0., SV_SQUARE_WIDTH - CROSSHAIR_SIZE);
    let y = ((1.0 - value.clamp(0., 1.)) * SV_SQUARE_HEIGHT - half)
        .clamp(0., SV_SQUARE_HEIGHT - CROSSHAIR_SIZE);

    let mut rings = Stack::new();
    rings.add_child(
        Rect::new()
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(half)))
            .with_border(Border::all(1.).with_border_fill(ColorU::new(0, 0, 0, 160)))
            .finish(),
    );
    rings.add_child(
        Container::new(
            Rect::new()
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(half - 1.)))
                .with_border(Border::all(2.).with_border_fill(OPAQUE_WHITE))
                .finish(),
        )
        .with_uniform_margin(1.)
        .finish(),
    );

    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(spacer(0., y))
        .with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_child(spacer(x, 0.))
                .with_child(
                    ConstrainedBox::new(rings.finish())
                        .with_width(CROSSHAIR_SIZE)
                        .with_height(CROSSHAIR_SIZE)
                        .finish(),
                )
                .finish(),
        )
        .finish()
}

/// The bar showing where the current hue sits on the strip.
fn render_hue_marker(hue: f32) -> Box<dyn Element> {
    let y = ((hue.clamp(0., 360.) / 360.0) * SV_SQUARE_HEIGHT - HUE_MARKER_HEIGHT / 2.)
        .clamp(0., SV_SQUARE_HEIGHT - HUE_MARKER_HEIGHT);

    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(spacer(0., y))
        .with_child(
            ConstrainedBox::new(
                Rect::new()
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(2.)))
                    .with_border(Border::all(2.).with_border_fill(OPAQUE_WHITE))
                    .finish(),
            )
            .with_width(HUE_STRIP_WIDTH)
            .with_height(HUE_MARKER_HEIGHT)
            .finish(),
        )
        .finish()
}

/// Turns a global pointer position into a 0–1 position inside the element saved under
/// `position_id`, or `None` if that element hasn't been laid out yet.
fn normalized_position_in(
    ctx: &EventContext,
    position_id: &str,
    position: Vector2F,
) -> Option<(f32, f32)> {
    let rect = ctx.element_position_by_id(position_id)?;
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    Some((
        ((position.x() - rect.origin_x()) / rect.width()).clamp(0., 1.),
        ((position.y() - rect.origin_y()) / rect.height()).clamp(0., 1.),
    ))
}

/// Quick-pick swatches shown in the picker: the Uncaged ember palette plus a neutral ramp and the
/// usual terminal hues, so most choices are one click.
const PRESET_COLORS: [u32; 14] = [
    0x15110c, 0x2c2620, 0x8c8378, 0xece6dc, 0xffffff, 0xffce4e, 0xff7a18, 0xff3b47, 0xff6b5e,
    0x8fd46e, 0x5fc9be, 0x74b4e0, 0xc8a9f0, 0xf090c0,
];

/// Splits a colour into hue (0–360), saturation and value (both 0–1).
///
/// The wheel works in HSV rather than HSL because the saturation/value square is an HSV plane:
/// full value across the top, black along the bottom. An HSL square would put white and black in
/// opposite corners and waste half its area.
fn coloru_to_hsv(color: ColorU) -> (f32, f32, f32) {
    let srgb = Srgb::new(
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
    );
    let hsv = Hsv::from_color(srgb);
    (
        hsv.hue.to_positive_degrees(),
        hsv.saturation.clamp(0.0, 1.0),
        hsv.value.clamp(0.0, 1.0),
    )
}

/// Rebuilds an opaque colour from hue (0–360), saturation and value (both 0–1).
fn hsv_to_coloru(hue: f32, saturation: f32, value: f32) -> ColorU {
    let hsv = Hsv::new(hue, saturation.clamp(0.0, 1.0), value.clamp(0.0, 1.0));
    let srgb = Srgb::from_color(hsv);
    ColorU::new(
        (srgb.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (srgb.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (srgb.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    )
}

// ── Page layout ──────────────────────────────────────────────────────────────

/// Below this width the preview stacks above the controls instead of sitting beside them.
const TWO_COLUMN_MIN_WIDTH: f32 = 760.;
/// The preview's column when it sits beside the controls. Wide enough for the custom editor's
/// three actions on one line under the preview.
const PREVIEW_COLUMN_WIDTH: f32 = 380.;
/// The terminal in the preview has a fixed height, so a background picture (which fills
/// whatever room it is given) has a box to fill.
const PREVIEW_TERMINAL_HEIGHT: f32 = 264.;
/// Background candidates offered in step 2 of "From image" (the image is reduced to this many
/// colours).
const MAX_BACKGROUND_CANDIDATES: usize = 5;
const CANDIDATE_TILE_HEIGHT: f32 = 40.;
const THUMBNAIL_WIDTH: f32 = 48.;
const THUMBNAIL_HEIGHT: f32 = 32.;
const NAME_INPUT_WIDTH: f32 = 220.;
const COLUMN_GAP: f32 = 16.;
/// The gap under each card, matching the rest of Settings.
const CARD_SPACING: f32 = 16.;
const SWATCH_SIZE: f32 = 24.;
const HEX_INPUT_WIDTH: f32 = 96.;
const SLIDER_WIDTH: f32 = 160.;
/// The fixed column each slider's value sits in, so values line up from row to row.
const SLIDER_VALUE_WIDTH: f32 = 40.;
const CONTROL_FONT_SIZE: f32 = 13.;
/// The inset between a segmented control's track and its segments.
const SEGMENT_TRACK_PADDING: f32 = 2.;
const PREVIEW_FONT_SIZE: f32 = 12.;
const STEP_BADGE_SIZE: f32 = 20.;
const PREVIEW_LINE_GAP: f32 = 3.;

/// What a [`slider_row`] edits and how it shows its value.
struct SliderRow {
    state: SliderStateHandle,
    value: u8,
    range: Range<f32>,
    /// The value as shown beside the slider, e.g. "80%".
    value_label: String,
    make_action: fn(f32) -> ThemeCreatorBodyAction,
}

/// One option of a [`segmented_control`].
struct Segment {
    label: &'static str,
    active: bool,
    mouse_state: MouseStateHandle,
    action: ThemeCreatorBodyAction,
}

/// The size and weight shared by every button on the page.
fn button_styles() -> UiComponentStyles {
    UiComponentStyles {
        font_size: Some(CONTROL_FONT_SIZE),
        font_weight: Some(Weight::Semibold),
        padding: Some(Coords::uniform(7.).left(14.).right(14.)),
        ..Default::default()
    }
}

/// A footer or row button. `Accent` is reserved for the one primary action (saving); every
/// other button is `Secondary`, so the accent colour never turns up as a big block.
fn action_button(
    label: &str,
    variant: ButtonVariant,
    mouse_state: MouseStateHandle,
    action: ThemeCreatorBodyAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .button(variant, mouse_state)
        .with_style(button_styles())
        .with_centered_text_label(label.into())
        .build()
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
}

/// A slider row: the label on the left; the slider and its current value, in a fixed-width
/// column, on the right. Every slider on the page shows its value the same way.
fn slider_row(
    label: &str,
    description: Option<String>,
    slider: SliderRow,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let SliderRow {
        state,
        value,
        range,
        value_label,
        make_action,
    } = slider;
    let slider = appearance
        .ui_builder()
        .slider(state)
        .with_range(range)
        .with_default_value(value as f32)
        .with_style(UiComponentStyles {
            width: Some(SLIDER_WIDTH),
            // Margin is 3. to add up with 7. padding on slider for a total of 10.
            margin: Some(Coords::default().top(3.).bottom(3.)),
            ..Default::default()
        })
        .on_drag(move |ctx, _, val| ctx.dispatch_typed_action(make_action(val)))
        .on_change(move |ctx, _, val| ctx.dispatch_typed_action(make_action(val)))
        .build()
        .finish();

    let value_text = ConstrainedBox::new(
        Align::new(
            Text::new_inline(value_label, appearance.ui_font_family(), CONTROL_FONT_SIZE)
                .with_color(settings_description_color(appearance))
                .finish(),
        )
        .right()
        .finish(),
    )
    .with_width(SLIDER_VALUE_WIDTH)
    .finish();

    let control = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(slider)
        .with_child(Container::new(value_text).with_margin_left(8.).finish())
        .finish();
    build_toggle_element(
        row_label(label, appearance),
        control,
        appearance,
        description,
    )
}

/// A compact segmented control: the options side by side in a quiet track, the selected one
/// raised. Neutral rather than accent-coloured, so the accent stays reserved for saving.
fn segmented_control(segments: Vec<Segment>, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let segment_radius =
        CornerRadius::with_all(Radius::Pixels(radius::SMALL - SEGMENT_TRACK_PADDING));
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center);
    for (index, segment) in segments.into_iter().enumerate() {
        let Segment {
            label,
            active,
            mouse_state,
            action,
        } = segment;
        let item = Hoverable::new(mouse_state, |state| {
            let text_color = if active || state.is_hovered() {
                theme.active_ui_text_color()
            } else {
                theme.nonactive_ui_text_color()
            };
            let weight = if active {
                Weight::Semibold
            } else {
                Weight::Normal
            };
            let container = Container::new(
                Text::new_inline(label, appearance.ui_font_family(), CONTROL_FONT_SIZE)
                    .with_color(text_color.into())
                    .with_style(Properties::default().weight(weight))
                    .finish(),
            )
            .with_vertical_padding(4.)
            .with_horizontal_padding(14.)
            .with_corner_radius(segment_radius);
            // Every segment carries a border, transparent unless selected, so selecting one
            // doesn't nudge its neighbours by a pixel.
            if active {
                container
                    .with_background(theme.surface_3())
                    .with_border(Border::all(1.).with_border_fill(theme.outline()))
                    .finish()
            } else {
                container
                    .with_border(Border::all(1.).with_border_color(ColorU::transparent_black()))
                    .finish()
            }
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish();
        row.add_child(if index == 0 {
            item
        } else {
            Container::new(item).with_margin_left(2.).finish()
        });
    }

    Container::new(row.finish())
        .with_uniform_padding(SEGMENT_TRACK_PADDING)
        .with_background(theme.surface_2())
        .with_corner_radius(radius::small())
        .finish()
}

/// One step of the "From image" flow: a settings card headed by its number, title and a line
/// saying what the step does. A step that isn't available yet is greyed out and shows only its
/// hint.
fn step_card(
    number: u8,
    title: &'static str,
    description: &'static str,
    enabled: bool,
    rows: Vec<Box<dyn Element>>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let title_color = if enabled {
        theme.active_ui_text_color()
    } else {
        theme.disabled_ui_text_color()
    };
    let description_color = if enabled {
        settings_description_color(appearance)
    } else {
        theme.disabled_ui_text_color().into_solid()
    };

    let badge = ConstrainedBox::new(
        Container::new(
            Align::new(
                Text::new_inline(number.to_string(), appearance.ui_font_family(), 12.)
                    .with_color(title_color.into())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .finish(),
        )
        .with_background(if enabled {
            theme.surface_3()
        } else {
            theme.surface_2()
        })
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(STEP_BADGE_SIZE / 2.)))
        .finish(),
    )
    .with_width(STEP_BADGE_SIZE)
    .with_height(STEP_BADGE_SIZE)
    .finish();

    let text = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Text::new_inline(title, appearance.ui_font_family(), SUBHEADER_FONT_SIZE)
                .with_color(title_color.into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_child(
            Container::new(
                Text::new(description, appearance.ui_font_family(), CONTENT_FONT_SIZE)
                    .with_color(description_color)
                    .finish(),
            )
            .with_margin_top(2.)
            .finish(),
        )
        .finish();

    let header = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(badge)
            .with_child(
                Shrinkable::new(1., Container::new(text).with_margin_left(10.).finish()).finish(),
            )
            .finish(),
    )
    .with_padding_bottom(HEADER_PADDING)
    .finish();

    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    column.add_child(header);
    if enabled {
        column.add_children(rows);
    }
    settings_card(column.finish(), appearance)
        .with_margin_bottom(CARD_SPACING)
        .finish()
}

/// Where programs commonly use each of the 8 normal terminal colours, in slot order (black, red,
/// green, yellow, blue, magenta, cyan, white). Shown under each colour in Advanced so people know
/// what they're changing. Kept to one short line each.
const ANSI_COLOR_USES: [&str; 8] = [
    "Dim text, unselected items, some prompt backgrounds",
    "Errors, failed commands, deleted lines in diffs",
    "Success, added lines in diffs, git branch names",
    "Warnings, modified files, search matches",
    "Directories in ls, links, info messages",
    "Prompt accents, keywords in some tools",
    "Paths, symlinks in ls, hints",
    "Regular output in many programs",
];

/// A small rounded thumbnail of a local picture.
fn image_thumbnail(path: &Path) -> Box<dyn Element> {
    ConstrainedBox::new(
        Image::new(
            AssetSource::LocalFile {
                path: path.to_string_lossy().into_owned(),
                content_version: None,
            },
            CacheOption::BySize,
        )
        .cover()
        .with_corner_radius(radius::small())
        .finish(),
    )
    .with_width(THUMBNAIL_WIDTH)
    .with_height(THUMBNAIL_HEIGHT)
    .finish()
}

/// Black or white, whichever stands out more on `background` — for the check mark on the
/// chosen background tile.
fn contrasting_mark_color(background: ColorU) -> ColorU {
    // Rec. 601 luma is plenty to tell a light tile from a dark one.
    let luma =
        0.299 * background.r as f32 + 0.587 * background.g as f32 + 0.114 * background.b as f32;
    if luma > 150. {
        ColorU::black()
    } else {
        ColorU::white()
    }
}

/// One section of the editor: a settings card with a small title (and optional description),
/// then its rows — the same card every other settings page uses.
fn section_card(
    title: &'static str,
    description: Option<&'static str>,
    rows: Vec<Box<dyn Element>>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    column.add_child(match description {
        Some(description) => render_sub_header_with_description(appearance, title, description),
        None => render_sub_header(appearance, title, None),
    });
    column.add_children(rows);
    settings_card(column.finish(), appearance)
        .with_margin_bottom(CARD_SPACING)
        .finish()
}

/// A row's label, styled like every other settings row's.
fn row_label(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Text::new_inline(
        text.to_string(),
        appearance.ui_font_family(),
        CONTROL_FONT_SIZE,
    )
    .with_color(appearance.theme().active_ui_text_color().into())
    .finish()
}

/// A small heading for a group of rows inside a card (the Advanced card's groups).
fn group_label(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(
            text.to_string(),
            appearance.ui_font_family(),
            CONTENT_FONT_SIZE,
        )
        .with_color(settings_description_color(appearance))
        .with_style(Properties::default().weight(Weight::Semibold))
        .finish(),
    )
    .with_margin_top(4.)
    .with_padding_bottom(10.)
    .finish()
}

fn hex_input_styles(appearance: &Appearance) -> UiComponentStyles {
    UiComponentStyles::default()
        .set_border_color(appearance.theme().outline().into())
        .set_font_family_id(appearance.monospace_font_family())
        .set_font_size(12.)
        .set_background(Fill::None)
        .set_border_radius(radius::small())
        .set_padding(Coords::uniform(6.).left(8.).right(8.))
        .set_border_width(1.)
}

fn name_input_styles(appearance: &Appearance) -> UiComponentStyles {
    UiComponentStyles::default()
        .set_border_color(appearance.theme().outline().into())
        .set_font_family_id(appearance.ui_font_family())
        .set_font_size(CONTROL_FONT_SIZE)
        .set_background(Fill::None)
        .set_border_radius(radius::small())
        .set_padding(Coords::uniform(8.).left(10.).right(10.))
        .set_border_width(1.)
}

/// A small terminal drawn entirely in `theme`: a tab strip with the active tab underlined in the
/// accent, a couple of commands and their output in the text and ANSI colours, the 16 ANSI
/// colours as chips, and the cursor block. The background is the theme's own fill, so its
/// gradient and opacity show as they will in the app.
///
/// This is the one place on the page with colours that don't come from the app's theme tokens:
/// it shows the theme being made, not the one the page is drawn in.
fn render_terminal_preview(theme: &WarpTheme, appearance: &Appearance) -> Box<dyn Element> {
    let font = appearance.monospace_font_family();
    let foreground = theme.foreground().into_solid();
    let dimmed = ColorU::new(foreground.r, foreground.g, foreground.b, 140);
    let rule = ColorU::new(foreground.r, foreground.g, foreground.b, 40);
    let accent = theme.accent().into_solid();
    let cursor = theme.cursor().into_solid();
    let terminal_colors = theme.terminal_colors();
    let normal = &terminal_colors.normal;
    let blue = ColorU::from(normal.blue);
    let green = ColorU::from(normal.green);
    let red = ColorU::from(normal.red);
    let yellow = ColorU::from(normal.yellow);
    let magenta = ColorU::from(normal.magenta);

    let span = |text: &str, color: ColorU| -> Box<dyn Element> {
        Text::new_inline(text.to_string(), font, PREVIEW_FONT_SIZE)
            .with_color(color)
            .finish()
    };
    let line = |spans: Vec<Box<dyn Element>>| -> Box<dyn Element> {
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        row.add_children(spans);
        Container::new(Clipped::new(row.finish()).finish())
            .with_padding_bottom(PREVIEW_LINE_GAP)
            .finish()
    };
    let prompt = |command: &str| -> Vec<Box<dyn Element>> {
        vec![
            span("~/uncaged", blue),
            span(" main", magenta),
            span(" ❯ ", green),
            span(command, foreground),
        ]
    };
    let tab = |label: &str, active: bool| -> Box<dyn Element> {
        let (text_color, underline) = if active {
            (foreground, accent)
        } else {
            (dimmed, ColorU::transparent_black())
        };
        Container::new(span(label, text_color))
            .with_horizontal_padding(10.)
            .with_padding_bottom(6.)
            .with_border(Border::bottom(2.).with_border_color(underline))
            .finish()
    };
    let chips = |colors: &AnsiColors| -> Box<dyn Element> {
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        let all = [
            colors.black,
            colors.red,
            colors.green,
            colors.yellow,
            colors.blue,
            colors.magenta,
            colors.cyan,
            colors.white,
        ];
        for (index, color) in all.into_iter().enumerate() {
            let chip = ConstrainedBox::new(
                Rect::new()
                    .with_background_color(ColorU::from(color))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(2.)))
                    .finish(),
            )
            .with_height(10.)
            .finish();
            let chip = if index == 0 {
                chip
            } else {
                Container::new(chip).with_margin_left(4.).finish()
            };
            row.add_child(Expanded::new(1., chip).finish());
        }
        Container::new(row.finish())
            .with_padding_bottom(4.)
            .finish()
    };

    let tabs = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::End)
            .with_child(tab("~/uncaged", true))
            .with_child(tab("zsh", false))
            .finish(),
    )
    .with_border(Border::bottom(1.).with_border_color(rule))
    .with_margin_bottom(10.)
    .finish();

    let mut cursor_line = prompt("");
    cursor_line.push(
        ConstrainedBox::new(Rect::new().with_background_color(cursor).finish())
            .with_width(7.)
            .with_height(14.)
            .finish(),
    );

    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    column.add_child(tabs);
    column.add_child(line(prompt("git status")));
    column.add_child(line(vec![span("On branch main", foreground)]));
    column.add_child(line(vec![span("  modified:  src/theme.rs", red)]));
    column.add_child(line(vec![span("  new file:  themes/ember.yaml", green)]));
    column.add_child(line(prompt("ls")));
    column.add_child(line(vec![
        span("app  ", blue),
        span("crates  ", blue),
        span("run.sh  ", green),
        span("README.md", foreground),
    ]));
    column.add_child(line(vec![
        span("warning: ", yellow),
        span("1 commit to push", foreground),
    ]));
    column.add_child(Container::new(chips(normal)).with_margin_top(6.).finish());
    column.add_child(chips(&terminal_colors.bright));
    column.add_child(
        Container::new(line(cursor_line))
            .with_margin_top(6.)
            .finish(),
    );

    // A background picture is drawn underneath, with the theme's background over it as a veil
    // at `100 - opacity` — the same way the app and the gallery thumbnails paint it. A picture
    // whose file is missing is skipped, along with its veil, rather than washing the preview out.
    let picture = theme.background_image().and_then(|image| {
        let source = image.source();
        let loadable = match &source {
            AssetSource::LocalFile { path, .. } => Path::new(path).exists(),
            _ => true,
        };
        loadable.then_some((source, image.opacity))
    });
    let veil_opacity = picture
        .as_ref()
        .map_or(100, |(_, opacity)| 100u8.saturating_sub(*opacity));

    let terminal = Container::new(Clipped::new(column.finish()).finish())
        .with_background(theme.background().with_opacity(veil_opacity))
        .with_corner_radius(radius::small())
        .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
        .with_horizontal_padding(12.)
        .with_padding_top(8.)
        .with_padding_bottom(10.)
        .finish();

    let mut layers = Stack::new();
    if let Some((source, _)) = picture {
        layers.add_child(
            Image::new(source, CacheOption::BySize)
                .cover()
                .with_corner_radius(radius::small())
                .finish(),
        );
    }
    layers.add_child(terminal);
    ConstrainedBox::new(layers.finish())
        .with_height(PREVIEW_TERMINAL_HEIGHT)
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
        let appearance = Appearance::as_ref(app);

        // The editor lives on a full settings page, which supplies its own scrolling. Letting the
        // content flow at its natural height (rather than boxing it into a fixed-height scroller
        // sized for the old modal) is what keeps the lower sections — appearance, background
        // opacity, Window, and Advanced — from being clipped off the bottom.
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.mode_tabs(appearance))
            .with_child(self.render_columns(app))
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
                    ctx.notify();
                } else {
                    self.set_theme_from_image_path(path.clone(), ctx);
                    ctx.notify();
                }
            }
            ThemeCreatorBodyAction::FilePickerCancelled => self.handle_file_picker_cancelled(ctx),
            ThemeCreatorBodyAction::SetMode(mode) => self.set_mode(*mode, ctx),
            ThemeCreatorBodyAction::ToggleGradient => self.toggle_gradient(ctx),
            ThemeCreatorBodyAction::SetLightAppearance(is_light) => {
                self.set_light_appearance(*is_light, ctx)
            }
            ThemeCreatorBodyAction::ToggleAdvanced => self.toggle_advanced(ctx),
            ThemeCreatorBodyAction::SetBackgroundOpacity(value) => {
                self.set_background_opacity(*value, ctx)
            }
            ThemeCreatorBodyAction::SetBackgroundImageOpacity(value) => {
                self.set_background_image_opacity(*value, ctx)
            }
            ThemeCreatorBodyAction::SetWindowOpacity(value) => self.set_window_opacity(*value, ctx),
            ThemeCreatorBodyAction::SetWindowBlurRadius(value) => {
                self.set_window_blur_radius(*value, ctx)
            }
            ThemeCreatorBodyAction::TogglePicker(index) => self.toggle_picker(*index, ctx),
            ThemeCreatorBodyAction::PickSaturationValue { x, y, start_drag } => {
                self.pick_saturation_value(*x, *y, *start_drag, ctx)
            }
            ThemeCreatorBodyAction::PickHue(position) => self.pick_hue(*position, ctx),
            ThemeCreatorBodyAction::EndPickerDrag => self.picker_dragging = false,
            ThemeCreatorBodyAction::SetPresetColor(rgb) => {
                let color = ColorU::from_u32((rgb << 8) | 0xFF);
                self.apply_color_to_open_slot(color, ctx);
                // Move the wheel onto the swatch the user just clicked.
                if let Some(index) = self.open_picker {
                    self.resync_picker_from_slot(index);
                }
            }
            ThemeCreatorBodyAction::RemoveBackgroundImage => {
                self.bg_image = None;
                ctx.notify();
            }
            ThemeCreatorBodyAction::PickBackgroundImage => {
                ctx.emit(ThemeCreatorBodyEvent::OpenFilePicker);
            }
            ThemeCreatorBodyAction::ShareTheme => self.share_theme(ctx),
            ThemeCreatorBodyAction::ToggleImageAsBackground => self.toggle_image_as_background(ctx),
            ThemeCreatorBodyAction::SetImageBackgroundOpacity(value) => {
                self.set_image_background_opacity(*value, ctx)
            }
            ThemeCreatorBodyAction::CustomizeGeneratedTheme => self.customize_generated_theme(ctx),
        }
    }
}

#[cfg(test)]
#[path = "theme_creator_body_tests.rs"]
mod tests;
