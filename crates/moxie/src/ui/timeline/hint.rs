//! The line or outline marking where a release would land.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::{ScrollPosition, UiGlobalTransform};
use bevy_fynix::WorldEntityMut;
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_ui::elements::Frame;
use moxie_ui::layout::logical_rect;
use moxie_ui::reactive::{BevyFynix, BevyUi, FynixHost};

use super::TrackViewport;

const OUTLINE_GROW: f32 = 2.0;
const OUTLINE_FILL_ALPHA: f32 = 0.15;

/// The hint node.
#[derive(Component)]
pub(super) struct Hint;

/// What the hint marks, in the viewport's content space.
pub(super) enum Shape {
    Insert(Rect),
    Merge { bounds: Rect, color: Color },
}

#[derive(EntityEvent)]
struct Show {
    entity: Entity,
    shape: Shape,
}

#[derive(EntityEvent)]
struct Hide {
    entity: Entity,
}

/// Shows and hides the [`Hint`] node.
#[derive(SystemParam)]
pub(super) struct HintNode<'w, 's> {
    hints: Query<'w, 's, Entity, With<Hint>>,
}

impl HintNode<'_, '_> {
    pub(super) fn show(&self, commands: &mut Commands, shape: Shape) {
        if let Ok(entity) = self.hints.single() {
            commands.trigger(Show { entity, shape });
        }
    }

    pub(super) fn hide(&self, commands: &mut Commands) {
        if let Ok(entity) = self.hints.single() {
            commands.trigger(Hide { entity });
        }
    }
}

impl Composer<FynixHost> for Hint {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let hint_z = Some(ui.theme.layer.drop_hint);

        let mut hint = ui.elem(elem!(
            Frame,
            position = PositionType::Absolute,
            display = Display::None,
            z = hint_z
        ));
        hint.insert((Pickable::IGNORE, self))
            .observe(on_show)
            .observe(on_hide);
        hint.handle()
    }
}

fn on_show(
    show: On<Show>,
    kernel: Res<BevyFynix>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform, &ScrollPosition),
        With<TrackViewport>,
    >,
    q_area: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut hints: Query<(
        &ChildOf,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    let Ok((viewport_node, viewport_transform, scroll)) =
        q_viewport.single()
    else {
        return;
    };
    let Ok((parent, mut node, mut background, mut border)) =
        hints.get_mut(show.entity)
    else {
        return;
    };
    let Ok((area_node, area_transform)) = q_area.get(parent.parent())
    else {
        return;
    };
    let theme = kernel.theme();

    let bounds = match &show.shape {
        Shape::Insert(bounds) => {
            background.0 = theme.color.accent;
            node.border = UiRect::ZERO;
            *bounds
        }
        Shape::Merge { bounds, color } => {
            background.0 = color.with_alpha(OUTLINE_FILL_ALPHA);
            node.border = UiRect::all(px(theme.space.edge));
            *border = BorderColor::all(*color);
            bounds.inflate(OUTLINE_GROW)
        }
    };

    // The area is the hint's parent, and the viewport scrolls under it.
    let to_area = logical_rect(viewport_node, viewport_transform).min
        - Vec2::new(0.0, scroll.y)
        - logical_rect(area_node, area_transform).min;
    node.display = Display::Flex;
    node.left = px(bounds.min.x + to_area.x);
    node.top = px(bounds.min.y + to_area.y);
    node.width = px(bounds.width());
    node.height = px(bounds.height());
}

fn on_hide(hide: On<Hide>, mut nodes: Query<&mut Node>) {
    if let Ok(mut node) = nodes.get_mut(hide.entity) {
        node.display = Display::None;
    }
}
