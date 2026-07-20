//! The browsable grid of themes available to install.
//!
//! A card per theme, rendered from the definition inlined in the catalogue, so the grid is fully
//! populated by a single request and nothing is downloaded until someone installs something.

use std::collections::HashSet;

use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, DispatchEventResult,
    Element, EventHandler, Fill, Flex, Icon, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, Shrinkable, Text, Wrap,
};
use warpui::platform::Cursor;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use crate::appearance::Appearance;
use crate::editor::{EditorView, Event as EditorEvent, SingleLineEditorOptions, TextOptions};
use crate::themes::theme;
use crate::themes::theme_gallery::{self, GalleryTheme};
use crate::user_config::{themes_dir, WarpConfig};

/// Card geometry. The preview inside a card is the same one the picker draws, scaled up a little
/// so a grid reads as a gallery rather than a list.
const CARD_WIDTH: f32 = 240.;
const CARD_PREVIEW_SCALE: f32 = 1.2;
const GRID_GUTTER: f32 = 14.;

/// Which themes the grid is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    All,
    System,
    Community,
}

impl GroupFilter {
    const ALL: [GroupFilter; 3] = [GroupFilter::All, GroupFilter::System, GroupFilter::Community];

    fn label(&self) -> &'static str {
        match self {
            GroupFilter::All => "All",
            GroupFilter::System => "System",
            GroupFilter::Community => "Community",
        }
    }

    /// The catalogue's `group` string this filter accepts, or `None` for everything.
    fn accepts(&self, group: &str) -> bool {
        match self {
            GroupFilter::All => true,
            GroupFilter::System => group == "system",
            GroupFilter::Community => group == "community",
        }
    }
}

/// What the gallery is currently doing.
enum LoadState {
    Loading,
    Loaded(Vec<GalleryTheme>),
    Failed(String),
}

pub enum ThemeGalleryBodyEvent {
    /// A theme was installed and should be loaded and applied.
    Installed { name: String },
    ShowErrorToast { message: String },
}

#[derive(Debug, Clone)]
pub enum ThemeGalleryBodyAction {
    Reload,
    SetFilter(GroupFilter),
    Install(String),
}

pub struct ThemeGalleryBody {
    state: LoadState,
    filter: GroupFilter,
    search_editor: ViewHandle<EditorView>,
    /// Slugs currently being downloaded, so a card can show progress and refuse a second click.
    installing: HashSet<String>,
    /// Slugs already on disk, so a card offers "Installed" rather than "Get".
    installed: HashSet<String>,
    filter_states: [MouseStateHandle; 3],
    retry_state: MouseStateHandle,
}

impl ThemeGalleryBody {
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
                ctx.notify();
            }
        });

        let mut body = Self {
            state: LoadState::Loading,
            filter: GroupFilter::All,
            search_editor,
            installing: HashSet::new(),
            installed: HashSet::new(),
            filter_states: Default::default(),
            retry_state: Default::default(),
        };
        body.refresh_installed();
        body.load(ctx);
        body
    }

    /// Re-reads the catalogue from the network.
    fn load(&mut self, ctx: &mut ViewContext<Self>) {
        self.state = LoadState::Loading;
        ctx.notify();

        ctx.spawn(
            async move { theme_gallery::fetch_index().await.map_err(|e| e.to_string()) },
            |me, result, ctx| {
                me.state = match result {
                    Ok(index) => LoadState::Loaded(index.themes),
                    Err(message) => LoadState::Failed(message),
                };
                me.refresh_installed();
                ctx.notify();
            },
        );
    }

    /// Whether this machine already has the theme, under any provenance.
    ///
    /// A gallery entry can already be present two ways: downloaded into `community/`, or shipped
    /// with the app. Every theme in the catalogue's `system/` folder is also a built-in, so
    /// checking only the download folder offered to "Get" themes the user already had.
    fn already_have(&self, gallery_theme: &GalleryTheme, app: &AppContext) -> bool {
        if self.installed.contains(&gallery_theme.slug) {
            return true;
        }
        WarpConfig::as_ref(app)
            .theme_config()
            .theme_items()
            .any(|(kind, _)| kind.to_string() == gallery_theme.name)
    }

    /// Notes which gallery themes are already downloaded, so cards show the right action.
    ///
    /// Reads the directory rather than tracking state, so a theme deleted by hand outside the app
    /// stops claiming to be installed.
    fn refresh_installed(&mut self) {
        self.installed.clear();
        let dir = themes_dir().join(theme::COMMUNITY_SUBFOLDER);
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    self.installed.insert(stem.to_owned());
                }
            }
        }
    }

    fn install(&mut self, slug: String, ctx: &mut ViewContext<Self>) {
        let LoadState::Loaded(themes) = &self.state else {
            return;
        };
        let Some(gallery_theme) = themes.iter().find(|t| t.slug == slug).cloned() else {
            return;
        };
        if self.installing.contains(&slug) || self.installed.contains(&slug) {
            return;
        }

        self.installing.insert(slug.clone());
        ctx.notify();

        let name = gallery_theme.name.clone();
        ctx.spawn(
            async move {
                theme_gallery::install(&gallery_theme, &themes_dir())
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            move |me, result, ctx| {
                me.installing.remove(&slug);
                me.refresh_installed();
                match result {
                    Ok(()) => ctx.emit(ThemeGalleryBodyEvent::Installed { name }),
                    Err(message) => ctx.emit(ThemeGalleryBodyEvent::ShowErrorToast {
                        message: format!("Couldn't install that theme: {message}"),
                    }),
                }
                ctx.notify();
            },
        );
    }

    /// The themes matching the current search text and group filter.
    fn visible<'a>(&'a self, themes: &'a [GalleryTheme], query: &str) -> Vec<&'a GalleryTheme> {
        themes
            .iter()
            .filter(|t| self.filter.accepts(&t.group) && t.matches(query))
            .collect()
    }
}

impl ThemeGalleryBody {
    fn render_filters(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);

        for (index, filter) in GroupFilter::ALL.into_iter().enumerate() {
            let active = self.filter == filter;
            row.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .button(
                            if active {
                                warpui::ui_components::button::ButtonVariant::Accent
                            } else {
                                warpui::ui_components::button::ButtonVariant::Secondary
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
                            ctx.dispatch_typed_action(ThemeGalleryBodyAction::SetFilter(filter))
                        })
                        .finish(),
                )
                .with_margin_right(8.)
                .finish(),
            );
        }

        let _ = theme;
        row.finish()
    }

    fn render_card(
        &self,
        gallery_theme: &GalleryTheme,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let slug = gallery_theme.slug.clone();
        let have_it = self.already_have(gallery_theme, app);
        let is_installing = self.installing.contains(&slug);

        // The preview is the card's subject, so it gets the full width and its own rounded frame.
        let preview = Container::new(theme::render_preview(
            &gallery_theme.definition,
            appearance.monospace_font_family(),
            Some(CARD_PREVIEW_SCALE),
        ))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        let name = Shrinkable::new(
            1.,
            Text::new_inline(gallery_theme.name.clone(), appearance.ui_font_family(), 13.)
                .with_color(theme.active_ui_text_color().into())
                .finish(),
        )
        .finish();

        // The action reads as a control when there is something to do, and as a quiet label when
        // there is not.
        let action: Box<dyn Element> = if have_it {
            Text::new_inline(
                "Installed".to_string(),
                appearance.ui_font_family(),
                11.,
            )
            .with_color(theme.disabled_text_color(theme.surface_2()).into_solid())
            .finish()
        } else {
            let label = if is_installing { "Installing…" } else { "Get" };
            Container::new(
                Text::new_inline(label.to_string(), appearance.ui_font_family(), 11.)
                    .with_color(theme.background().into_solid())
                    .finish(),
            )
            .with_horizontal_padding(10.)
            .with_vertical_padding(4.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
            .with_background(theme.accent())
            .finish()
        };

        let mut card = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        card.add_child(preview);
        card.add_child(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    // Without this the name and the action render flush against each other.
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(name)
                    .with_child(action)
                    .finish(),
            )
            .with_margin_top(10.)
            .finish(),
        );

        let body = Container::new(card.finish())
            .with_uniform_padding(10.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
            .with_background(theme.surface_2())
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .finish();

        // Nothing to do for a theme that is already here, so it is not clickable either.
        let element: Box<dyn Element> = if have_it || is_installing {
            body
        } else {
            EventHandler::new(body)
                .on_left_mouse_down(move |ctx, _, _| {
                    ctx.dispatch_typed_action(ThemeGalleryBodyAction::Install(slug.clone()));
                    DispatchEventResult::StopPropagation
                })
                .finish()
        };

        ConstrainedBox::new(element).with_width(CARD_WIDTH).finish()
    }

    fn render_grid(
        &self,
        themes: &[&GalleryTheme],
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        if themes.is_empty() {
            return Container::new(
                Text::new_inline(
                    "No themes match that search.".to_string(),
                    appearance.ui_font_family(),
                    13.,
                )
                .with_color(
                    appearance
                        .theme()
                        .disabled_text_color(appearance.theme().background())
                        .into_solid(),
                )
                .finish(),
            )
            .with_margin_top(20.)
            .finish();
        }

        let mut grid = Wrap::row()
            .with_spacing(GRID_GUTTER)
            .with_run_spacing(GRID_GUTTER)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);
        grid.extend(themes.iter().map(|t| self.render_card(t, appearance, app)));
        grid.finish()
    }

    fn render_message(&self, message: &str, retry: bool, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Start);
        column.add_child(
            Text::new_inline(message.to_string(), appearance.ui_font_family(), 13.)
                .with_color(theme.disabled_text_color(theme.background()).into_solid())
                .finish(),
        );
        if retry {
            column.add_child(
                Container::new(
                    appearance
                        .ui_builder()
                        .button(
                            warpui::ui_components::button::ButtonVariant::Secondary,
                            self.retry_state.clone(),
                        )
                        .with_style(UiComponentStyles {
                            font_size: Some(12.),
                            padding: Some(Coords::uniform(8.)),
                            ..Default::default()
                        })
                        .with_centered_text_label("Try again".into())
                        .build()
                        .with_cursor(Cursor::PointingHand)
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(ThemeGalleryBodyAction::Reload)
                        })
                        .finish(),
                )
                .with_margin_top(10.)
                .finish(),
            );
        }
        Container::new(column.finish()).with_margin_top(16.).finish()
    }
}

impl Entity for ThemeGalleryBody {
    type Event = ThemeGalleryBodyEvent;
}

impl TypedActionView for ThemeGalleryBody {
    type Action = ThemeGalleryBodyAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ThemeGalleryBodyAction::Reload => self.load(ctx),
            ThemeGalleryBodyAction::SetFilter(filter) => {
                self.filter = *filter;
                ctx.notify();
            }
            ThemeGalleryBodyAction::Install(slug) => self.install(slug.clone(), ctx),
        }
    }
}

impl View for ThemeGalleryBody {
    fn ui_name() -> &'static str {
        "ThemeGalleryBody"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let query = self.search_editor.as_ref(app).buffer_text(app);
        let query = query.trim();

        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        // Search box.
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

        match &self.state {
            LoadState::Loading => {
                column.add_child(self.render_message("Loading themes…", false, &appearance))
            }
            // The error already reads as a sentence; prefixing it doubles the subject.
            LoadState::Failed(message) => {
                column.add_child(self.render_message(message, true, &appearance))
            }
            LoadState::Loaded(themes) => {
                let visible = self.visible(themes, query);
                column.add_child(self.render_grid(&visible, &appearance, app));
            }
        }

        Container::new(column.finish()).finish()
    }
}
