//! The landing hint: the line or outline that marks where a release
//! would land.
//!
//! A [`Landing`] node sits hidden in the track area. A drag triggers
//! [`ShowLanding`] (bounds in content space) or [`HideLanding`], and
//! the observers here place it, so callers skip the scroll math.

use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform};
use bevy_fynix::WorldEntityMut;
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_ui::elements::Frame;
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::{BevyUi, FynixHost};

use super::TrackViewport;

/// How far the merge outline sits outside the bounds it marks.
const OUTLINE_GROW: f32 = 2.0;
/// The outline's fill alpha.
const OUTLINE_FILL_ALPHA: f32 = 0.15;

/// Registers the observers that place and hide the hints.
pub(super) fn plugin(app: &mut App) {
    app.add_observer(on_show).add_observer(on_hide);
}

/// A landing hint node, and which kind it is.
#[derive(Component, Clone, Copy)]
pub(super) enum Landing {
    /// The slim line of an insert.
    Line,
    /// The box around the node a merge lands on.
    Outline,
}

/// Shows the hint a release would land on. Bounds are in the
/// viewport's content space, where the `Placed`s live.
#[derive(Event)]
pub(super) enum ShowLanding {
    Insert(Rect),
    Merge { bounds: Rect, color: Color },
}

/// Hides both hints.
#[derive(Event)]
pub(super) struct HideLanding;

impl Composer<FynixHost> for Landing {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let theme = ui.theme;
        let hint_z = Some(theme.layer.drop_hint);

        let mut hint = match self {
            Landing::Line => ui.elem(elem!(
                Frame,
                position = PositionType::Absolute,
                display = Display::None,
                background = theme.color.accent,
                z = hint_z
            )),
            Landing::Outline => {
                let merge = theme.palette.purple;
                ui.elem(elem!(
                    Frame,
                    position = PositionType::Absolute,
                    display = Display::None,
                    background = merge.with_alpha(OUTLINE_FILL_ALPHA),
                    border = px(theme.space.edge),
                    border_color = merge,
                    z = hint_z
                ))
            }
        };
        hint.insert(Pickable::IGNORE).insert(self);
        hint.handle()
    }
}

/// Places the hint `show` calls for and hides the other.
pub(super) fn on_show(
    show: On<ShowLanding>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    q_area: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut hints: Query<(
        &Landing,
        &ChildOf,
        &mut Node,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
    )>,
) {
    let Ok((viewport_node, viewport_transform, scroll)) =
        q_viewport.single()
    else {
        return;
    };
    let viewport_min =
        logical_rect(viewport_node, viewport_transform).min;

    for (landing, parent, mut node, background, border) in &mut hints
    {
        let bounds = match (landing, &*show) {
            (Landing::Line, ShowLanding::Insert(bounds)) => {
                Some(*bounds)
            }
            (
                Landing::Outline,
                ShowLanding::Merge { bounds, color },
            ) => {
                if let Some(mut background) = background {
                    background.0 =
                        color.with_alpha(OUTLINE_FILL_ALPHA);
                }
                if let Some(mut border) = border {
                    *border = BorderColor::all(*color);
                }
                Some(bounds.inflate(OUTLINE_GROW))
            }
            _ => None,
        };
        let Some(bounds) = bounds else {
            node.display = Display::None;
            continue;
        };
        let Ok((area_node, area_transform)) =
            q_area.get(parent.parent())
        else {
            continue;
        };

        // The area is the hint's parent, so its origin is the hint's
        // zero; the viewport's content scrolls under it.
        let to_area = viewport_min
            - Vec2::new(0.0, scroll.y)
            - logical_rect(area_node, area_transform).min;
        node.display = Display::Flex;
        node.left = px(bounds.min.x + to_area.x);
        node.top = px(bounds.min.y + to_area.y);
        node.width = px(bounds.width());
        node.height = px(bounds.height());
    }
}

/// Hides both hints.
pub(super) fn on_hide(
    _: On<HideLanding>,
    mut hints: Query<&mut Node, With<Landing>>,
) {
    for mut node in &mut hints {
        node.display = Display::None;
    }
}
