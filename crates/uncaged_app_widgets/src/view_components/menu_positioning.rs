use pathfinder_geometry::vector::{vec2f, Vector2F};
use uncaged_ui::elements::{AnchorPair, ChildAnchor, OffsetType, ParentAnchor, YAxisAnchor};
use uncaged_ui::AppContext;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub enum MenuPositioning {
    /// Position floating input menus above the input box -- corresponds
    /// to the regular blocklist.
    #[default]
    AboveInputBox,

    /// Position floating input menus below the input box -- corresponds
    /// to the inverted blocklist.
    BelowInputBox,
}

impl MenuPositioning {
    pub fn completion_suggestions_y_anchor(&self) -> AnchorPair<YAxisAnchor> {
        self.y_anchor()
    }

    pub fn history_y_anchor(&self) -> AnchorPair<YAxisAnchor> {
        self.y_anchor()
    }

    pub fn history_y_offset(&self) -> OffsetType {
        match *self {
            // Uncaged: a small gap so the menu floats instead of sitting on the input's border.
            MenuPositioning::AboveInputBox => OffsetType::Pixel(-6.),
            MenuPositioning::BelowInputBox => OffsetType::Pixel(-11.),
        }
    }

    pub fn command_xray_y_anchor(&self) -> AnchorPair<YAxisAnchor> {
        self.y_anchor()
    }

    pub fn workflows_info_y_anchor(&self) -> AnchorPair<YAxisAnchor> {
        self.y_anchor()
    }

    pub fn voltron_parent_anchor(&self) -> ParentAnchor {
        match *self {
            MenuPositioning::AboveInputBox => ParentAnchor::BottomLeft,
            MenuPositioning::BelowInputBox => ParentAnchor::TopLeft,
        }
    }

    pub fn voltron_child_anchor(&self) -> ChildAnchor {
        match *self {
            MenuPositioning::AboveInputBox => ChildAnchor::BottomLeft,
            MenuPositioning::BelowInputBox => ChildAnchor::TopLeft,
        }
    }

    pub fn voltron_offset(&self) -> Vector2F {
        match *self {
            MenuPositioning::AboveInputBox => vec2f(11., -11.),
            MenuPositioning::BelowInputBox => vec2f(11., -66.),
        }
    }

    pub fn y_anchor(&self) -> AnchorPair<YAxisAnchor> {
        match *self {
            MenuPositioning::AboveInputBox => {
                AnchorPair::new(YAxisAnchor::Top, YAxisAnchor::Bottom)
            }
            MenuPositioning::BelowInputBox => {
                AnchorPair::new(YAxisAnchor::Bottom, YAxisAnchor::Top)
            }
        }
    }
}

impl MenuPositioningProvider for MenuPositioning {
    fn menu_position(&self, _app: &AppContext) -> MenuPositioning {
        *self
    }
}

/// Implement this trait to provide whether menus like autocomplete, voltron, etc
/// should be positionined above or below the input.
pub trait MenuPositioningProvider {
    fn menu_position(&self, app: &AppContext) -> MenuPositioning;

    fn inline_menu_position(&self, _inline_menu_height: f32, _app: &AppContext) -> MenuPositioning {
        MenuPositioning::AboveInputBox
    }
}
