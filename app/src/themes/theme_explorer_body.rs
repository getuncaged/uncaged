//! One place to see every theme — the ones that ship, the ones you downloaded, the ones you made,
//! and the ones you could have.
//!
//! The catalogue's `system/` themes are the same ones bundled with the app, so they are never
//! offered as downloads; they appear as what they are, already here. What the gallery genuinely
//! adds is community themes, and those sit alongside your own rather than in a separate place, so
//! "find me a theme" is one screen rather than two.

use std::collections::HashSet;
use std::path::PathBuf;

use settings::Setting as _;
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
use warp_core::ui::theme::WarpTheme;

use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::settings::{active_theme_kind, ThemeSettings};
use crate::themes::theme::SelectedSystemThemes;
use crate::themes::theme::{self, ThemeGroup, ThemeKind};
use crate::themes::theme_gallery::{self, GalleryTheme};
use crate::user_config::{load_theme_configs, themes_dir, WarpConfig};
use crate::report_if_error;

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
struct Entry {
    name: String,
    definition: WarpTheme,
    origin: Origin,
    /// How to select it. `None` for a theme that is not installed yet.
    kind: Option<ThemeKind>,
    /// Where it lives, for deletion. `None` unless it is a file this user may remove.
    path: Option<PathBuf>,
    /// Catalogue slug, for installing.
    slug: Option<String>,
}

/// How the community half of the explorer is doing. Local themes need none of this — they are
/// always there.
enum CatalogueState {
    Loading,
    Loaded(Vec<GalleryTheme>),
    Failed(String),
}

pub enum ThemeExplorerBodyEvent {
    ShowErrorToast { message: String },
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
}

pub struct ThemeExplorerBody {
    catalogue: CatalogueState,
    filter: Filter,
    search_editor: ViewHandle<EditorView>,
    installing: HashSet<String>,
    /// The theme whose delete button has been armed, identified by path. Removing a theme is not
    /// undoable, so it takes a second, deliberate click rather than a single stray one.
    ///
    /// Keyed on the path rather than the name because names are not unique — a downloaded theme
    /// can share a built-in's name, and the built-in sorts first.
    pending_delete: Option<PathBuf>,
    filter_states: [MouseStateHandle; 5],
    retry_state: MouseStateHandle,
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
        ctx.subscribe_to_view(&search_editor, |_me, _, event, ctx| {
            if let EditorEvent::Edited(_) = event {
                ctx.notify();
            }
        });

        let mut body = Self {
            catalogue: CatalogueState::Loading,
            filter: Filter::default(),
            search_editor,
            installing: HashSet::new(),
            pending_delete: None,
            filter_states: Default::default(),
            retry_state: Default::default(),
        };
        body.load(ctx);
        body
    }

    fn load(&mut self, ctx: &mut ViewContext<Self>) {
        self.catalogue = CatalogueState::Loading;
        ctx.notify();

        ctx.spawn(
            async move { theme_gallery::fetch_index().await.map_err(|e| e.to_string()) },
            |me, result, ctx| {
                me.catalogue = match result {
                    Ok(index) => CatalogueState::Loaded(index.themes),
                    Err(message) => CatalogueState::Failed(message),
                };
                ctx.notify();
            },
        );
    }

    /// Everything the explorer can show, installed first and offers last.
    fn entries(&self, app: &AppContext) -> Vec<Entry> {
        let mut entries = Vec::new();
        let mut present: HashSet<String> = HashSet::new();

        for (kind, definition) in WarpConfig::as_ref(app).theme_config().theme_items() {
            let name = kind.to_string();
            present.insert(name.clone());

            let origin = match kind.group() {
                ThemeGroup::System => Origin::System,
                ThemeGroup::Community => Origin::Downloaded,
                ThemeGroup::Mine => Origin::Mine,
            };
            let path = match kind {
                ThemeKind::Custom(custom) | ThemeKind::CustomBase16(custom) => Some(custom.path()),
                _ => None,
            };

            entries.push(Entry {
                name,
                definition: definition.clone(),
                origin,
                kind: Some(kind.clone()),
                path,
                slug: None,
            });
        }

        entries.sort_by(|a, b| a.origin.cmp_key().cmp(&b.origin.cmp_key()).then(a.name.cmp(&b.name)));

        if let CatalogueState::Loaded(catalogue) = &self.catalogue {
            for gallery_theme in catalogue {
                // A catalogue entry already on this machine is not an offer — it is the thing
                // itself, and was listed above.
                if present.contains(&gallery_theme.name) {
                    continue;
                }
                entries.push(Entry {
                    name: gallery_theme.name.clone(),
                    definition: gallery_theme.definition.clone(),
                    origin: Origin::Available,
                    kind: None,
                    path: None,
                    slug: Some(gallery_theme.slug.clone()),
                });
            }
        }

        entries
    }

    fn install(&mut self, slug: String, ctx: &mut ViewContext<Self>) {
        let CatalogueState::Loaded(catalogue) = &self.catalogue else {
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

        ThemeSettings::handle(ctx).update(ctx, |theme_settings, ctx| {
            if syncing_with_os {
                let updated = match system_theme {
                    SystemTheme::Light => SelectedSystemThemes {
                        light: kind.clone(),
                        dark: selected.dark.clone(),
                    },
                    SystemTheme::Dark => SelectedSystemThemes {
                        light: selected.light.clone(),
                        dark: kind.clone(),
                    },
                };
                report_if_error!(theme_settings.selected_system_themes.set_value(updated, ctx));
            } else {
                report_if_error!(theme_settings.theme_kind.set_value(kind.clone(), ctx));
            }
        });
        ctx.notify();
    }

    /// Removes a theme's files, then reloads so it disappears from the grid.
    fn delete(&mut self, path: PathBuf, ctx: &mut ViewContext<Self>) {
        self.pending_delete = None;

        let Some(entry) = self
            .entries(ctx)
            .into_iter()
            .find(|e| e.path.as_deref() == Some(path.as_path()))
        else {
            return;
        };
        if !entry.origin.deletable() {
            return;
        }
        // Take the theme off first if it is the one in use, so the window is not left pointing at
        // a file that no longer exists.
        if entry.kind.as_ref() == Some(&active_theme_kind(ThemeSettings::as_ref(ctx), ctx)) {
            self.apply(ThemeKind::default(), ctx);
        }

        // A theme's image is only removed when it lives in the themes dir. A theme can legitimately
        // point at a wallpaper elsewhere on the disk, and deleting the theme must not take the
        // user's own picture with it.
        if let Some(image) = entry.definition.background_image() {
            if let AssetSource::LocalFile { path: image_path, .. } = image.source() {
                let image_path = PathBuf::from(image_path);
                if image_path.starts_with(themes_dir()) {
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
        is_active: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let installing = entry
            .slug
            .as_ref()
            .is_some_and(|slug| self.installing.contains(slug));
        let armed = self.pending_delete.is_some() && self.pending_delete == entry.path;

        let preview = Container::new(theme::render_preview(
            &entry.definition,
            appearance.monospace_font_family(),
            Some(CARD_PREVIEW_SCALE),
        ))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        // The right-hand control says what this card will do if you click it.
        let action: Box<dyn Element> = if entry.origin == Origin::Available {
            let label = if installing { "Installing…" } else { "Get" };
            pill(label, theme.accent(), theme.background().into_solid(), appearance)
        } else if is_active {
            pill(
                "In use",
                theme.surface_3(),
                theme.main_text_color(theme.surface_3()).into_solid(),
                appearance,
            )
        } else {
            Text::new_inline("Use".to_string(), appearance.ui_font_family(), 11.)
                .with_color(theme.accent().into_solid())
                .finish()
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
        card.add_child(Container::new(footer.finish()).with_margin_top(10.).finish());
        card.add_child(
            Container::new(
                Text::new_inline(entry.origin.label().to_string(), appearance.ui_font_family(), 10.)
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
            .with_border(Border::all(if is_active { 2. } else { 1. }).with_border_fill(
                if is_active {
                    theme.accent()
                } else {
                    theme.outline()
                },
            ))
            .finish();

        // Clicking the card does the obvious thing: install it if it is not here, use it if it is.
        let element: Box<dyn Element> = match (&entry.kind, &entry.slug) {
            _ if installing || is_active => body,
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
            ThemeExplorerBodyAction::Reload => self.load(ctx),
            ThemeExplorerBodyAction::SetFilter(filter) => {
                self.filter = *filter;
                self.pending_delete = None;
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
        let query = self.search_editor.as_ref(app).buffer_text(app);
        let query = query.trim().to_lowercase();
        // What is genuinely in effect, which is not `theme_kind` when syncing with the OS.
        let active = active_theme_kind(ThemeSettings::as_ref(app), app);

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        column.add_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            ConstrainedBox::new(
                                Icon::new("bundled/svg/find.svg", theme.active_ui_detail()).finish(),
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
            Container::new(self.render_filters(&appearance))
                .with_margin_bottom(16.)
                .finish(),
        );

        let entries = self.entries(app);
        let visible: Vec<&Entry> = entries
            .iter()
            .filter(|e| {
                self.filter.accepts(e.origin)
                    && (query.is_empty() || e.name.to_lowercase().contains(&query))
            })
            .collect();

        if visible.is_empty() {
            let message = match &self.catalogue {
                CatalogueState::Loading if self.filter == Filter::Available => "Loading themes…",
                _ => "No themes match that search.",
            };
            column.add_child(
                Container::new(
                    Text::new_inline(message.to_string(), appearance.ui_font_family(), 13.)
                        .with_color(theme.disabled_text_color(theme.background()).into_solid())
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
            grid.extend(visible.iter().map(|entry| {
                let is_active = entry.kind.as_ref() == Some(&active);
                self.render_card(entry, is_active, &appearance)
            }));
            column.add_child(grid.finish());
        }

        // A failed catalogue only costs the "Available" themes, so it is reported under the grid
        // rather than replacing everything the user already has.
        if let CatalogueState::Failed(message) = &self.catalogue {
            let mut notice = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);
            notice.add_child(
                Text::new_inline(
                    format!("{message} Themes already on this machine are unaffected."),
                    appearance.ui_font_family(),
                    12.,
                )
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
            column.add_child(Container::new(notice.finish()).with_margin_top(20.).finish());
        }

        Container::new(column.finish()).finish()
    }
}
