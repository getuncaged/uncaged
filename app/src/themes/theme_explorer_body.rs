//! One place to see every theme — the ones that ship, the ones you downloaded, the ones you made,
//! and the ones you could have.
//!
//! The catalogue's `system/` themes are the same ones bundled with the app, so they are never
//! offered as downloads; they appear as what they are, already here. What the gallery genuinely
//! adds is community themes, and those sit alongside your own rather than in a separate place, so
//! "find me a theme" is one screen rather than two.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use settings::Setting as _;
use warp_core::ui::theme::WarpTheme;
use warpui::assets::asset_cache::AssetSource;
use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult,
    Element, EventHandler, Fill, Flex, Icon, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, Shrinkable, Text, Wrap,
};
use warpui::platform::{Cursor, SystemTheme};
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Entity, SingletonEntity, TypedActionView, UpdateModel, View, ViewContext,
    ViewHandle,
};

use crate::appearance::{Appearance, AppearanceManager};
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::report_if_error;
use crate::settings::{active_theme_kind, ThemeSettings};
use crate::themes::theme::{self, SelectedSystemThemes, ThemeGroup, ThemeKind};
use crate::themes::theme_background_image;
use crate::themes::theme_gallery::{
    self, GalleryTheme, ThemeGalleryCatalogue, ThemeGalleryCatalogueEvent,
};
use crate::user_config::{load_theme_configs, themes_dir, WarpConfig, WarpConfigUpdateEvent};

const CARD_WIDTH: f32 = 240.;
const CARD_PREVIEW_SCALE: f32 = 1.2;
const GRID_GUTTER: f32 = 14.;

/// Where a theme in the explorer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Ships with the app. Cannot be deleted.
    System,
    /// Installed from the community gallery.
    Downloaded,
    /// Written by this user.
    Mine,
    /// In the gallery but not on this machine.
    Available,
}

impl Origin {
    /// Deleting a theme you did not create and cannot re-obtain would be a trap, so only the two
    /// recoverable kinds are removable: a download can be fetched again, and your own is yours.
    fn deletable(&self) -> bool {
        matches!(self, Origin::Downloaded | Origin::Mine)
    }

    /// Shown under a theme's name. Names are not unique — a downloaded theme may share a built-in's
    /// name — so without this two cards would read identically.
    fn label(&self) -> &'static str {
        match self {
            Origin::System => "System",
            Origin::Downloaded => "Downloaded",
            Origin::Mine => "Yours",
            Origin::Available => "Community",
        }
    }
}

/// Which slice of the explorer is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    System,
    Downloaded,
    Mine,
    Available,
}

impl Filter {
    const ALL: [Filter; 5] = [
        Filter::All,
        Filter::System,
        Filter::Downloaded,
        Filter::Mine,
        Filter::Available,
    ];

    fn label(&self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::System => "System",
            Filter::Downloaded => "Downloaded",
            Filter::Mine => "Yours",
            Filter::Available => "Available",
        }
    }

    fn accepts(&self, origin: Origin) -> bool {
        match self {
            Filter::All => true,
            Filter::System => origin == Origin::System,
            Filter::Downloaded => origin == Origin::Downloaded,
            Filter::Mine => origin == Origin::Mine,
            Filter::Available => origin == Origin::Available,
        }
    }
}

/// A single card's worth of theme, whether it is here or merely offered.
///
/// Built once whenever the themes on disk or the catalogue change — never per frame — so a hover
/// or a keystroke in the search box repaints the grid without walking, cloning and sorting every
/// theme again.
struct Entry {
    name: String,
    /// `name`, lowercased once for the search box.
    name_lower: String,
    definition: Arc<WarpTheme>,
    origin: Origin,
    /// How to select it. `None` for a theme that is not installed yet.
    kind: Option<ThemeKind>,
    /// Where it lives, for deletion. `None` unless it is a file this user may remove.
    path: Option<PathBuf>,
    /// Catalogue slug, for installing.
    slug: Option<String>,
    /// What the card draws behind its miniature terminal.
    preview: PreviewImage,
}

/// The background image of a card, resolved ahead of painting so a paint never touches the disk.
#[derive(Clone)]
enum PreviewImage {
    /// Nothing to draw: no image, an image that isn't on disk, or a community theme not yet
    /// installed.
    None,
    /// The small image to draw.
    Ready(AssetSource),
    /// A local image whose thumbnail is still being looked up, by its full path. The card shows
    /// its colours alone until then.
    Pending(String),
}

impl PreviewImage {
    fn source(&self) -> Option<AssetSource> {
        match self {
            PreviewImage::Ready(source) => Some(source.clone()),
            PreviewImage::None | PreviewImage::Pending(_) => None,
        }
    }
}

pub enum ThemeExplorerBodyEvent {
    ShowErrorToast {
        message: String,
    },
    /// Picking mode is over — a theme was chosen for the slot, or the pick was cancelled. The host
    /// takes the user back to where the pick started.
    PickFinished,
}

/// What a card's right-hand control says, which is what clicking the card does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardControl {
    /// Not on this machine yet: clicking installs it.
    Get,
    Installing,
    /// The theme already in the place being chosen for — in use, or the light or dark theme
    /// while picking.
    Current(&'static str),
    /// Clicking selects it, for the place the label names.
    Use(&'static str),
}

impl CardControl {
    /// Which control a card shows. `is_current` means the card is the theme already in the place
    /// being chosen for: the active theme normally, the slot's theme while picking.
    fn for_card(
        origin: Origin,
        installing: bool,
        is_current: bool,
        picking: Option<SystemTheme>,
    ) -> Self {
        match (origin, picking) {
            (Origin::Available, _) if installing => CardControl::Installing,
            (Origin::Available, _) => CardControl::Get,
            (Origin::System | Origin::Downloaded | Origin::Mine, None) if is_current => {
                CardControl::Current("In use")
            }
            (Origin::System | Origin::Downloaded | Origin::Mine, None) => CardControl::Use("Use"),
            (Origin::System | Origin::Downloaded | Origin::Mine, Some(SystemTheme::Light))
                if is_current =>
            {
                CardControl::Current("Light theme")
            }
            (Origin::System | Origin::Downloaded | Origin::Mine, Some(SystemTheme::Dark))
                if is_current =>
            {
                CardControl::Current("Dark theme")
            }
            (Origin::System | Origin::Downloaded | Origin::Mine, Some(SystemTheme::Light)) => {
                CardControl::Use("Use for light")
            }
            (Origin::System | Origin::Downloaded | Origin::Mine, Some(SystemTheme::Dark)) => {
                CardControl::Use("Use for dark")
            }
        }
    }

    /// Whether clicking the card does anything. The theme already in use is inert, but while
    /// picking, choosing the slot's current theme again is a fine way to keep it and go back.
    fn is_clickable(&self, picking: Option<SystemTheme>) -> bool {
        match self {
            CardControl::Get | CardControl::Use(_) => true,
            CardControl::Installing => false,
            CardControl::Current(_) => picking.is_some(),
        }
    }
}

/// The line at the top of the gallery while it is choosing a theme for `slot`.
fn picking_banner(slot: SystemTheme) -> String {
    let system = if cfg!(target_os = "macos") {
        "your Mac"
    } else {
        "your system"
    };
    match slot {
        SystemTheme::Light => {
            format!("Choose your light theme — shown when {system} is in light mode.")
        }
        SystemTheme::Dark => {
            format!("Choose your dark theme — shown when {system} is in dark mode.")
        }
    }
}

#[derive(Debug, Clone)]
pub enum ThemeExplorerBodyAction {
    Reload,
    SetFilter(Filter),
    Install(String),
    Apply(ThemeKind),
    ConfirmDelete(PathBuf),
    Delete(PathBuf),
    CancelDelete,
    /// Leave picking mode without changing anything.
    CancelPick,
}

pub struct ThemeExplorerBody {
    /// The community catalogue as last seen on the shared [`ThemeGalleryCatalogue`].
    catalogue: Option<Arc<Vec<GalleryTheme>>>,
    /// Nothing to offer yet, and a fetch is under way.
    catalogue_loading: bool,
    /// Why the last fetch failed, if it did.
    catalogue_error: Option<String>,
    /// The saved catalogue is on show because this run's fetch failed.
    catalogue_stale: bool,
    /// Whether the gallery has been shown yet. The catalogue and the card images are only looked
    /// at from then on, so building Settings at startup reads nothing and fetches nothing.
    activated: bool,
    filter: Filter,
    search_editor: ViewHandle<EditorView>,
    /// The search box's text, trimmed and lowercased.
    query: String,
    /// Every card, in display order.
    entries: Vec<Entry>,
    /// Indices into `entries` that pass the filter and the search.
    visible: Vec<usize>,
    /// Card images already resolved off the main thread, keyed by the full image's path, so a
    /// rebuild after an install or delete doesn't look them all up again.
    resolved_images: HashMap<String, Option<AssetSource>>,
    /// Full image paths being resolved right now.
    resolving_images: HashSet<String>,
    installing: HashSet<String>,
    /// The theme whose delete button has been armed, identified by path. Removing a theme is not
    /// undoable, so it takes a second, deliberate click rather than a single stray one.
    ///
    /// Keyed on the path rather than the name because names are not unique — a downloaded theme
    /// can share a built-in's name, and the built-in sorts first.
    pending_delete: Option<PathBuf>,
    filter_states: [MouseStateHandle; 5],
    retry_state: MouseStateHandle,
    /// The "Sync with OS" slot being chosen, when the gallery was opened to pick the light or dark
    /// theme. `None` is the ordinary gallery, where Use sets the theme in effect.
    picking: Option<SystemTheme>,
    cancel_pick_state: MouseStateHandle,
}

impl ThemeExplorerBody {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let search_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_font_size(appearance),
                    ..Default::default()
                },
                ctx,
            )
        });
        ctx.subscribe_to_view(&search_editor, |me, _, event, ctx| {
            if let EditorEvent::Edited(_) = event {
                let query = me.search_editor.as_ref(ctx).buffer_text(ctx);
                me.query = query.trim().to_lowercase();
                me.recompute_visible();
                ctx.notify();
            }
        });

        ctx.subscribe_to_model(&WarpConfig::handle(ctx), |me, _, event, ctx| {
            if let WarpConfigUpdateEvent::Themes = event {
                me.rebuild_entries(ctx);
            }
        });
        // The "In use" marker follows the active theme, which is read while painting; this only
        // asks for the repaint.
        ctx.subscribe_to_model(&ThemeSettings::handle(ctx), |_, _, _, ctx| ctx.notify());

        let mut body = Self {
            catalogue: None,
            catalogue_loading: true,
            catalogue_error: None,
            catalogue_stale: false,
            activated: false,
            filter: Filter::default(),
            search_editor,
            query: String::new(),
            entries: Vec::new(),
            visible: Vec::new(),
            resolved_images: HashMap::new(),
            resolving_images: HashSet::new(),
            installing: HashSet::new(),
            pending_delete: None,
            filter_states: Default::default(),
            retry_state: Default::default(),
            picking: None,
            cancel_pick_state: Default::default(),
        };
        // Local themes are already in memory, so the cards for them cost nothing to build. Their
        // images and the community catalogue wait until the gallery is shown.
        body.rebuild_entries(ctx);
        body
    }

    /// Starts or ends picking mode — choosing the theme for one "Sync with OS" slot rather than
    /// the theme in effect.
    pub fn set_picking(&mut self, picking: Option<SystemTheme>, ctx: &mut ViewContext<Self>) {
        if self.picking != picking {
            self.picking = picking;
            self.pending_delete = None;
            ctx.notify();
        }
    }

    /// Called when the gallery is shown. The first time, it starts listening to the shared
    /// catalogue — which shows the saved copy and refreshes it once per run — and starts resolving
    /// card images.
    pub fn activate(&mut self, ctx: &mut ViewContext<Self>) {
        if self.activated {
            return;
        }
        self.activated = true;

        let catalogue = ThemeGalleryCatalogue::handle(ctx);
        ctx.subscribe_to_model(&catalogue, |me, _, event, ctx| {
            let ThemeGalleryCatalogueEvent::Updated = event;
            me.sync_catalogue(ctx);
        });
        catalogue.update(ctx, |catalogue, ctx| catalogue.ensure_loaded(ctx));
        self.sync_catalogue(ctx);
    }

    /// Copies what the shared catalogue knows into this view, rebuilding the cards only when the
    /// list itself changed.
    fn sync_catalogue(&mut self, ctx: &mut ViewContext<Self>) {
        let catalogue = ThemeGalleryCatalogue::as_ref(ctx);
        self.catalogue_loading = catalogue.is_loading();
        self.catalogue_stale = catalogue.is_stale();
        self.catalogue_error = catalogue.error().map(str::to_owned);
        let themes = catalogue.themes().cloned();
        let unchanged = match (&self.catalogue, &themes) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            // Only the loading or error state moved.
            ctx.notify();
            return;
        }
        self.catalogue = themes;
        self.rebuild_entries(ctx);
    }

    /// Everything the explorer can show, installed first and offers last.
    fn rebuild_entries(&mut self, ctx: &mut ViewContext<Self>) {
        let mut entries = Vec::new();
        let mut present: HashSet<String> = HashSet::new();

        for (kind, definition) in WarpConfig::as_ref(ctx).theme_config().theme_items() {
            // The referral reward themes are promotional leftovers: Uncaged has no referrals.
            // They are always present in the theme map, so without this every user would see two
            // System cards for rewards nobody can earn — and the explorer can neither install nor
            // delete them, so they'd be inert.
            if matches!(
                kind,
                ThemeKind::SentReferralReward | ThemeKind::ReceivedReferralReward
            ) {
                continue;
            }

            let name = kind.to_string();
            present.insert(name.clone());

            let path = match kind {
                ThemeKind::Custom(custom) | ThemeKind::CustomBase16(custom) => Some(custom.path()),
                _ => None,
            };
            let origin = match kind.group() {
                ThemeGroup::System => Origin::System,
                ThemeGroup::Community => Origin::Downloaded,
                ThemeGroup::Mine => Origin::Mine,
            };
            let preview = self.preview_for(definition);

            entries.push(Entry {
                name_lower: name.to_lowercase(),
                name,
                definition: Arc::new(definition.clone()),
                origin,
                kind: Some(kind.clone()),
                path,
                slug: None,
                preview,
            });
        }

        entries.sort_by(|a, b| {
            a.origin
                .cmp_key()
                .cmp(&b.origin.cmp_key())
                .then(a.name.cmp(&b.name))
        });

        if let Some(catalogue) = &self.catalogue {
            for gallery_theme in catalogue.iter() {
                // A catalogue entry already on this machine is not an offer — it is the thing
                // itself, and was listed above.
                if present.contains(&gallery_theme.name) {
                    continue;
                }
                entries.push(Entry {
                    name: gallery_theme.name.clone(),
                    name_lower: gallery_theme.name.to_lowercase(),
                    definition: Arc::new(gallery_theme.definition.clone()),
                    origin: Origin::Available,
                    kind: None,
                    path: None,
                    slug: Some(gallery_theme.slug.clone()),
                    // Not on disk yet, so there is no image to draw — only its colours.
                    preview: PreviewImage::None,
                });
            }
        }

        self.entries = entries;
        self.recompute_visible();
        self.resolve_pending_images(ctx);
        ctx.notify();
    }

    /// What a card can draw right now for `definition`'s background, without touching the disk.
    fn preview_for(&self, definition: &WarpTheme) -> PreviewImage {
        let Some(image) = definition.background_image() else {
            return PreviewImage::None;
        };
        match image.source() {
            AssetSource::LocalFile { path, .. } => match self.resolved_images.get(&path) {
                Some(Some(source)) => PreviewImage::Ready(source.clone()),
                Some(None) => PreviewImage::None,
                None => PreviewImage::Pending(path),
            },
            source => PreviewImage::Ready(crate::themes::default_themes::preview_source(source)),
        }
    }

    /// Looks up, off the main thread, the thumbnails for any cards still waiting on one —
    /// creating the thumbnails that are missing.
    fn resolve_pending_images(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.activated {
            return;
        }
        let mut to_resolve: Vec<(String, Arc<WarpTheme>)> = Vec::new();
        for entry in &self.entries {
            if let PreviewImage::Pending(path) = &entry.preview {
                if self.resolving_images.insert(path.clone()) {
                    to_resolve.push((path.clone(), entry.definition.clone()));
                }
            }
        }
        if to_resolve.is_empty() {
            return;
        }

        ctx.spawn(
            async move {
                to_resolve
                    .into_iter()
                    .map(|(path, definition)| (path, theme::resolve_preview_image(&definition)))
                    .collect::<Vec<_>>()
            },
            |me, resolved, ctx| {
                for (path, source) in resolved {
                    me.resolving_images.remove(&path);
                    let preview = match &source {
                        Some(source) => PreviewImage::Ready(source.clone()),
                        None => PreviewImage::None,
                    };
                    for entry in &mut me.entries {
                        if matches!(&entry.preview, PreviewImage::Pending(pending) if *pending == path)
                        {
                            entry.preview = preview.clone();
                        }
                    }
                    me.resolved_images.insert(path, source);
                }
                ctx.notify();
            },
        );
    }

    /// Applies the filter and the search to the entries.
    fn recompute_visible(&mut self) {
        let filter = self.filter;
        let query = &self.query;
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                filter.accepts(e.origin) && (query.is_empty() || e.name_lower.contains(query))
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn install(&mut self, slug: String, ctx: &mut ViewContext<Self>) {
        let Some(catalogue) = &self.catalogue else {
            return;
        };
        let Some(gallery_theme) = catalogue.iter().find(|t| t.slug == slug).cloned() else {
            return;
        };
        if !self.installing.insert(slug.clone()) {
            return;
        }
        ctx.notify();

        ctx.spawn(
            async move {
                theme_gallery::install(&gallery_theme, &themes_dir())
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            move |me, result, ctx| {
                me.installing.remove(&slug);
                match result {
                    Ok(()) => me.reload_themes(ctx),
                    Err(message) => ctx.emit(ThemeExplorerBodyEvent::ShowErrorToast {
                        message: format!("Couldn't install that theme: {message}"),
                    }),
                }
                ctx.notify();
            },
        );
    }

    /// Selects a theme, writing to whichever setting is actually in effect.
    ///
    /// With "Sync with OS" on, the live theme comes from `selected_system_themes` and `theme_kind`
    /// is ignored entirely — so writing only `theme_kind`, as this used to, marked the card in use
    /// while the window kept its old theme. The slot written is the one matching the current system
    /// appearance, so the change is visible immediately rather than the next time the OS flips.
    fn apply(&mut self, kind: ThemeKind, ctx: &mut ViewContext<Self>) {
        let settings = ThemeSettings::as_ref(ctx);
        let syncing_with_os = *settings.use_system_theme.value();
        let selected = settings.selected_system_themes.value().clone();
        let system_theme = ctx.system_theme();

        // Picking fills the slot it was opened for, whatever the OS is showing right now.
        let picked_slot = self.picking.take();
        let slot = picked_slot.or(syncing_with_os.then_some(system_theme));

        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| match slot {
            Some(slot) => {
                report_if_error!(theme_settings
                    .selected_system_themes
                    .set_value(selected.with_theme_for(slot, kind.clone()), ctx));
            }
            None => {
                report_if_error!(theme_settings.theme_kind.set_value(kind.clone(), ctx));
            }
        });

        // Writing the setting persists the choice, and the AppearanceManager repaints from the
        // settings — unless a transient (preview) theme is pinned over them, which would keep the
        // window on its old theme while the card flips to "In use". Dropping any transient theme
        // makes the window follow the settings from here on, including an OS appearance flip when
        // Sync with OS is on. (Pinning the chosen theme as the transient one, as this used to,
        // made the window ignore every later change to the theme settings.)
        AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
            appearance_manager.clear_transient_theme(ctx);
        });
        if picked_slot.is_some() {
            ctx.emit(ThemeExplorerBodyEvent::PickFinished);
        }
        ctx.notify();
    }

    /// Resets every place a theme is selected — the plain choice and both Sync-with-OS slots — back
    /// to the default wherever it equals `deleted`.
    ///
    /// All three settings persist regardless of which is active, so a slot still pointing at a
    /// just-deleted theme would activate a missing file the moment Sync is toggled or the OS flips.
    fn clear_theme_everywhere(&mut self, deleted: &ThemeKind, ctx: &mut ViewContext<Self>) {
        let plain = ThemeSettings::as_ref(ctx).theme_kind.value().clone();
        let selected = ThemeSettings::as_ref(ctx)
            .selected_system_themes
            .value()
            .clone();

        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
            if plain == *deleted {
                report_if_error!(theme_settings
                    .theme_kind
                    .set_value(ThemeKind::default(), ctx));
            }
            if selected.light == *deleted || selected.dark == *deleted {
                let updated = SelectedSystemThemes {
                    light: if selected.light == *deleted {
                        ThemeKind::default()
                    } else {
                        selected.light.clone()
                    },
                    dark: if selected.dark == *deleted {
                        ThemeKind::default()
                    } else {
                        selected.dark.clone()
                    },
                };
                report_if_error!(theme_settings
                    .selected_system_themes
                    .set_value(updated, ctx));
            }
        });
        ctx.notify();
    }

    /// Removes a theme's files, then reloads so it disappears from the grid.
    fn delete(&mut self, path: PathBuf, ctx: &mut ViewContext<Self>) {
        self.pending_delete = None;

        let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.path.as_deref() == Some(path.as_path()))
        else {
            return;
        };
        if !entry.origin.deletable() {
            return;
        }
        let deleted_kind = entry.kind.clone();
        let definition = entry.definition.clone();
        // Take the theme off wherever it is selected, so no slot is left pointing at a file that is
        // about to vanish. With Sync-with-OS on, the light and dark slots are independent and the
        // deleted theme may sit in either or both — not only the one currently showing — so every
        // reference is cleared, not just the active one.
        if let Some(deleted) = deleted_kind {
            self.clear_theme_everywhere(&deleted, ctx);
        }

        // A theme's image is only removed when it genuinely lives in the themes dir. A theme can
        // legitimately point at a wallpaper elsewhere on the disk, and deleting the theme must not
        // take the user's own picture with it. Its preview thumbnail, which is ours and sits beside
        // it, goes too so it does not linger as an orphan.
        if let Some(image) = definition.background_image() {
            if let AssetSource::LocalFile {
                path: image_path, ..
            } = image.source()
            {
                // A theme installed again at this path brings its own image; look it up afresh.
                self.resolved_images.remove(&image_path);
                let image_path = PathBuf::from(image_path);
                if path_is_within(&image_path, &themes_dir()) {
                    let _ =
                        std::fs::remove_file(theme_background_image::thumbnail_path(&image_path));
                    let _ = std::fs::remove_file(image_path);
                }
            }
        }

        if let Err(error) = std::fs::remove_file(&path) {
            ctx.emit(ThemeExplorerBodyEvent::ShowErrorToast {
                message: format!("Couldn't delete that theme: {error}"),
            });
            return;
        }

        self.reload_themes(ctx);
    }

    /// Re-reads the themes dir so the grid matches what is on disk.
    fn reload_themes(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.spawn(
            async move { load_theme_configs(&themes_dir()) },
            |_me, loaded_themes, ctx| {
                ctx.update_model(&WarpConfig::handle(ctx), move |warp_config, ctx| {
                    warp_config.update_theme_config(loaded_themes, ctx);
                });
                ctx.notify();
            },
        );
    }
}

impl Origin {
    /// Installed themes read before offers, and within that, the ones you shipped with first.
    fn cmp_key(&self) -> u8 {
        match self {
            Origin::System => 0,
            Origin::Downloaded => 1,
            Origin::Mine => 2,
            Origin::Available => 3,
        }
    }
}

impl ThemeExplorerBody {
    fn render_filters(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (index, filter) in Filter::ALL.into_iter().enumerate() {
            let active = self.filter == filter;
            row.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .button(
                            if active {
                                ButtonVariant::Accent
                            } else {
                                ButtonVariant::Secondary
                            },
                            self.filter_states[index].clone(),
                        )
                        .with_style(UiComponentStyles {
                            font_size: Some(12.),
                            padding: Some(Coords::uniform(8.)),
                            ..Default::default()
                        })
                        .with_centered_text_label(filter.label().into())
                        .build()
                        .with_cursor(Cursor::PointingHand)
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(ThemeExplorerBodyAction::SetFilter(filter))
                        })
                        .finish(),
                )
                .with_margin_right(8.)
                .finish(),
            );
        }
        row.finish()
    }

    fn render_card(
        &self,
        entry: &Entry,
        is_current: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let installing = entry
            .slug
            .as_ref()
            .is_some_and(|slug| self.installing.contains(slug));
        let armed = self.pending_delete.is_some() && self.pending_delete == entry.path;

        let preview = Container::new(theme::render_preview_with_image(
            &entry.definition,
            entry.preview.source(),
            appearance.monospace_font_family(),
            Some(CARD_PREVIEW_SCALE),
        ))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        // The right-hand control says what this card will do if you click it.
        let control = CardControl::for_card(entry.origin, installing, is_current, self.picking);
        let action: Box<dyn Element> = match control {
            CardControl::Get | CardControl::Installing => pill(
                if control == CardControl::Installing {
                    "Installing…"
                } else {
                    "Get"
                },
                theme.accent(),
                theme.background().into_solid(),
                appearance,
            ),
            CardControl::Current(label) => pill(
                label,
                theme.surface_3(),
                theme.main_text_color(theme.surface_3()).into_solid(),
                appearance,
            ),
            CardControl::Use(label) => {
                Text::new_inline(label.to_string(), appearance.ui_font_family(), 11.)
                    .with_color(theme.accent().into_solid())
                    .finish()
            }
        };

        let footer = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    Text::new_inline(entry.name.clone(), appearance.ui_font_family(), 13.)
                        .with_color(theme.active_ui_text_color().into())
                        .finish(),
                )
                .finish(),
            )
            .with_child(action);

        let mut card = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        card.add_child(preview);
        card.add_child(
            Container::new(footer.finish())
                .with_margin_top(10.)
                .finish(),
        );
        card.add_child(
            Container::new(
                Text::new_inline(
                    entry.origin.label().to_string(),
                    appearance.ui_font_family(),
                    10.,
                )
                .with_color(theme.disabled_text_color(theme.surface_2()).into_solid())
                .finish(),
            )
            .with_margin_top(2.)
            .finish(),
        );

        // Deleting is destructive and irreversible. The resting state is a quiet trash icon, and
        // acting on it swaps in an explicit confirm button rather than removing anything — so a
        // stray click on a small target can never destroy a theme.
        if entry.origin.deletable() {
            if let Some(path) = entry.path.clone() {
                let control: Box<dyn Element> = if armed {
                    let confirm_path = path.clone();
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Container::new(
                                EventHandler::new(pill(
                                    "Delete",
                                    theme.ui_error_color().into(),
                                    theme.background().into_solid(),
                                    appearance,
                                ))
                                .on_left_mouse_down(move |ctx, _, _| {
                                    ctx.dispatch_typed_action(ThemeExplorerBodyAction::Delete(
                                        confirm_path.clone(),
                                    ));
                                    DispatchEventResult::StopPropagation
                                })
                                .finish(),
                            )
                            .with_margin_right(6.)
                            .finish(),
                        )
                        .with_child(
                            EventHandler::new(
                                Text::new_inline(
                                    "Cancel".to_string(),
                                    appearance.ui_font_family(),
                                    11.,
                                )
                                .with_color(
                                    theme.disabled_text_color(theme.surface_2()).into_solid(),
                                )
                                .finish(),
                            )
                            .on_left_mouse_down(|ctx, _, _| {
                                ctx.dispatch_typed_action(ThemeExplorerBodyAction::CancelDelete);
                                DispatchEventResult::StopPropagation
                            })
                            .finish(),
                        )
                        .finish()
                } else {
                    EventHandler::new(
                        ConstrainedBox::new(
                            Icon::new(
                                "bundled/svg/trash-02.svg",
                                theme.disabled_text_color(theme.surface_2()),
                            )
                            .finish(),
                        )
                        .with_width(14.)
                        .with_height(14.)
                        .finish(),
                    )
                    .on_left_mouse_down(move |ctx, _, _| {
                        ctx.dispatch_typed_action(ThemeExplorerBodyAction::ConfirmDelete(
                            path.clone(),
                        ));
                        DispatchEventResult::StopPropagation
                    })
                    .finish()
                };

                card.add_child(Container::new(control).with_margin_top(8.).finish());
            }
        }

        let body = Container::new(card.finish())
            .with_uniform_padding(10.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
            .with_background(theme.surface_2())
            .with_border(
                Border::all(if is_current { 2. } else { 1. }).with_border_fill(if is_current {
                    theme.accent()
                } else {
                    theme.outline()
                }),
            )
            .finish();

        // Clicking the card does the obvious thing: install it if it is not here, use it if it is.
        let element: Box<dyn Element> = match (&entry.kind, &entry.slug) {
            _ if !control.is_clickable(self.picking) => body,
            (Some(kind), _) => {
                let kind = kind.clone();
                EventHandler::new(body)
                    .on_left_mouse_down(move |ctx, _, _| {
                        ctx.dispatch_typed_action(ThemeExplorerBodyAction::Apply(kind.clone()));
                        DispatchEventResult::StopPropagation
                    })
                    .finish()
            }
            (None, Some(slug)) => {
                let slug = slug.clone();
                EventHandler::new(body)
                    .on_left_mouse_down(move |ctx, _, _| {
                        ctx.dispatch_typed_action(ThemeExplorerBodyAction::Install(slug.clone()));
                        DispatchEventResult::StopPropagation
                    })
                    .finish()
            }
            _ => body,
        };

        ConstrainedBox::new(element).with_width(CARD_WIDTH).finish()
    }
}

impl ThemeExplorerBody {
    /// The strip across the top of the gallery while it is choosing a light or dark theme: what is
    /// being chosen, and the way out.
    fn render_picking_banner(
        &self,
        slot: SystemTheme,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let cancel = appearance
            .ui_builder()
            .button(ButtonVariant::Secondary, self.cancel_pick_state.clone())
            .with_style(UiComponentStyles {
                font_size: Some(12.),
                padding: Some(Coords::uniform(6.).left(12.).right(12.)),
                ..Default::default()
            })
            .with_centered_text_label("Cancel".into())
            .build()
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| ctx.dispatch_typed_action(ThemeExplorerBodyAction::CancelPick))
            .finish();

        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Shrinkable::new(
                        1.,
                        Text::new(picking_banner(slot), appearance.ui_font_family(), 13.)
                            .with_color(theme.active_ui_text_color().into())
                            .finish(),
                    )
                    .finish(),
                )
                .with_child(Container::new(cancel).with_margin_left(12.).finish())
                .finish(),
        )
        .with_uniform_padding(12.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.accent()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .with_margin_bottom(14.)
        .finish()
    }
}

/// A small filled label.
fn pill(
    label: &str,
    background: warp_core::ui::theme::Fill,
    text: warpui::color::ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    Container::new(
        Text::new_inline(label.to_string(), appearance.ui_font_family(), 11.)
            .with_color(text)
            .finish(),
    )
    .with_horizontal_padding(10.)
    .with_vertical_padding(4.)
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
    .with_background(background)
    .finish()
}

impl Entity for ThemeExplorerBody {
    type Event = ThemeExplorerBodyEvent;
}

impl TypedActionView for ThemeExplorerBody {
    type Action = ThemeExplorerBodyAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ThemeExplorerBodyAction::Reload => {
                ThemeGalleryCatalogue::handle(ctx)
                    .update(ctx, |catalogue, ctx| catalogue.refresh(ctx));
            }
            ThemeExplorerBodyAction::SetFilter(filter) => {
                self.filter = *filter;
                self.pending_delete = None;
                self.recompute_visible();
                ctx.notify();
            }
            ThemeExplorerBodyAction::Install(slug) => {
                // Any half-armed delete is abandoned the moment attention moves elsewhere.
                self.pending_delete = None;
                self.install(slug.clone(), ctx);
            }
            ThemeExplorerBodyAction::Apply(kind) => {
                self.pending_delete = None;
                self.apply(kind.clone(), ctx);
            }
            ThemeExplorerBodyAction::ConfirmDelete(path) => {
                self.pending_delete = Some(path.clone());
                ctx.notify();
            }
            ThemeExplorerBodyAction::Delete(path) => self.delete(path.clone(), ctx),
            ThemeExplorerBodyAction::CancelDelete => {
                self.pending_delete = None;
                ctx.notify();
            }
            ThemeExplorerBodyAction::CancelPick => {
                self.set_picking(None, ctx);
                ctx.emit(ThemeExplorerBodyEvent::PickFinished);
            }
        }
    }
}

impl View for ThemeExplorerBody {
    fn ui_name() -> &'static str {
        "ThemeExplorerBody"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let theme_settings = ThemeSettings::as_ref(app);
        // The card to mark: the slot's theme while picking, otherwise what is genuinely in
        // effect — which is not `theme_kind` when syncing with the OS.
        let current = match self.picking {
            Some(slot) => theme_settings
                .selected_system_themes
                .value()
                .for_appearance(slot)
                .clone(),
            None => active_theme_kind(theme_settings, app),
        };

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        if let Some(slot) = self.picking {
            column.add_child(self.render_picking_banner(slot, appearance));
        }

        column.add_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            ConstrainedBox::new(
                                Icon::new("bundled/svg/find.svg", theme.active_ui_detail())
                                    .finish(),
                            )
                            .with_height(12.)
                            .with_width(12.)
                            .finish(),
                        )
                        .with_margin_right(6.)
                        .finish(),
                    )
                    .with_child(
                        Shrinkable::new(
                            1.,
                            appearance
                                .ui_builder()
                                .text_input(self.search_editor.clone())
                                .with_style(UiComponentStyles {
                                    background: Some(Fill::None),
                                    border_width: Some(0.),
                                    ..Default::default()
                                })
                                .build()
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .with_margin_bottom(10.)
            .finish(),
        );

        column.add_child(
            Container::new(self.render_filters(appearance))
                .with_margin_bottom(16.)
                .finish(),
        );

        // With Sync with OS on, "Use" fills the slot for the appearance the OS is in right now —
        // say so, or picking a theme in light mode and then seeing another at night is a mystery.
        if self.picking.is_none() && *theme_settings.use_system_theme.value() {
            let slot = match app.system_theme() {
                SystemTheme::Light => "light",
                SystemTheme::Dark => "dark",
            };
            column.add_child(
                Container::new(
                    Text::new(
                        format!(
                            "Sync with OS is on, so Use sets your {slot} theme. Choose the other \
                             one in Appearance."
                        ),
                        appearance.ui_font_family(),
                        12.,
                    )
                    .with_color(theme.disabled_text_color(theme.background()).into_solid())
                    .finish(),
                )
                .with_margin_bottom(14.)
                .finish(),
            );
        }

        if self.visible.is_empty() {
            // An empty tab with no search isn't a failed search: say what the tab is waiting for.
            let message = if !self.query.is_empty() {
                "No themes match that search."
            } else {
                match self.filter {
                    Filter::Available if self.catalogue_loading => "Loading themes…",
                    Filter::Available => {
                        "You already have every theme in the community gallery. Make one in \
                         Theme creator and share it with the community."
                    }
                    Filter::Downloaded => {
                        "Nothing downloaded yet. Themes you get from the community gallery show \
                         up here."
                    }
                    Filter::Mine => {
                        "You haven't made a theme yet. Theme creator is where to start."
                    }
                    Filter::All | Filter::System => "No themes to show.",
                }
            };
            column.add_child(
                Container::new(
                    // A paragraph, so the longer hints wrap on a narrow window.
                    appearance
                        .ui_builder()
                        .paragraph(message.to_string())
                        .with_style(UiComponentStyles {
                            font_color: Some(
                                theme.disabled_text_color(theme.background()).into_solid(),
                            ),
                            font_size: Some(13.),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .with_margin_top(20.)
                .finish(),
            );
        } else {
            let mut grid = Wrap::row()
                .with_spacing(GRID_GUTTER)
                .with_run_spacing(GRID_GUTTER)
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            grid.extend(self.visible.iter().map(|&index| {
                let entry = &self.entries[index];
                let is_current = entry.kind.as_ref() == Some(&current);
                self.render_card(entry, is_current, appearance)
            }));
            column.add_child(grid.finish());
        }

        // A failed catalogue only costs the "Available" themes, so it is reported under the grid
        // rather than replacing everything the user already has. When the saved copy is on show,
        // the failure only means it may be out of date.
        let notice = match (self.catalogue_error.as_deref(), self.catalogue_stale) {
            (Some(_), true) => Some(
                "Couldn't refresh the community themes, so these are from your last visit."
                    .to_string(),
            ),
            (Some(message), false) if self.catalogue.is_none() => Some(format!(
                "{message} Themes already on this machine are unaffected."
            )),
            (Some(_), false) | (None, _) => None,
        };
        if let Some(text) = notice {
            let mut notice = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);
            notice.add_child(
                Text::new_inline(text, appearance.ui_font_family(), 12.)
                    .with_color(theme.disabled_text_color(theme.background()).into_solid())
                    .finish(),
            );
            notice.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .button(ButtonVariant::Secondary, self.retry_state.clone())
                        .with_style(UiComponentStyles {
                            font_size: Some(12.),
                            padding: Some(Coords::uniform(8.)),
                            ..Default::default()
                        })
                        .with_centered_text_label("Try again".into())
                        .build()
                        .with_cursor(Cursor::PointingHand)
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(ThemeExplorerBodyAction::Reload)
                        })
                        .finish(),
                )
                .with_margin_top(8.)
                .finish(),
            );
            column.add_child(
                Container::new(notice.finish())
                    .with_margin_top(20.)
                    .finish(),
            );
        }

        Container::new(column.finish()).finish()
    }
}

/// True only when `path` genuinely resolves inside `base`, with `..` and symlinks accounted for.
///
/// A lexical `starts_with` is not enough: `<base>/../../etc/x` passes it component-for-component
/// yet resolves outside `base`. Canonicalising both first collapses those, so a theme whose image
/// path has been crafted to escape the themes dir cannot trick delete into removing a file
/// elsewhere. Canonicalisation needs the paths to exist, which they do at delete time; if either
/// can't be resolved the answer is a safe `false`.
fn path_is_within(path: &Path, base: &Path) -> bool {
    match (path.canonicalize(), base.canonicalize()) {
        (Ok(path), Ok(base)) => path.starts_with(base),
        _ => false,
    }
}

#[cfg(test)]
#[path = "theme_explorer_body_tests.rs"]
mod tests;
