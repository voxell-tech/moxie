//! The line or outline marking where a release would land.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy_fynix::views::{BehaviorExt as _, frame};
use bevy_fynix::{AnyView, Bevy, Theme};
use moxie_ui::layout::logical_rect;
use moxie_ui::theme::EditorTheme;

use super::TrackViewport;
use super::reorder::scrolled;

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
    hints: Query<'w, 's, (Entity, &'static ChildOf), With<Hint>>,
}

impl HintNode<'_, '_> {
    pub(super) fn show(&self, commands: &mut Commands, shape: Shape) {
        if let Ok((entity, _)) = self.hints.single() {
            commands.trigger(Show { entity, shape });
        }
    }

    pub(super) fn hide(&self, commands: &mut Commands) {
        if let Ok((entity, _)) = self.hints.single() {
            commands.trigger(Hide { entity });
        }
    }

    /// The node the hint floats over: the viewport's parent, which
    /// does not scroll.
    pub(super) fn area(&self) -> Option<Entity> {
        self.hints.single().ok().map(|(_, parent)| parent.parent())
    }
}

/// The offset from the viewport's content space to `area`'s own,
/// with the viewport scrolled as it is.
pub(super) fn content_to_area(
    viewport: (&ComputedNode, &UiGlobalTransform),
    area: (&ComputedNode, &UiGlobalTransform),
) -> Vec2 {
    logical_rect(viewport.0, viewport.1).min
        - Vec2::new(0.0, scrolled(viewport.0))
        - logical_rect(area.0, area.1).min
}

/// The hint, hidden until something shows it. It floats over its
/// parent and ignores the pointer.
pub(super) fn hint() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let node = cx.build(
            frame()
                .position(PositionType::Absolute)
                .display(Display::None)
                .z(Some(cx.theme().layer.drop_hint))
                .tagged(Hint),
        );
        cx.world
            .entity_mut(node)
            .insert(Pickable::IGNORE)
            .observe(on_show)
            .observe(on_hide);
        node
    })
}

fn on_show(
    show: On<Show>,
    theme: Res<Theme<EditorTheme>>,
    q_viewport: Query<
        (&ComputedNode, &UiGlobalTransform),
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
    let Ok(viewport) = q_viewport.single() else {
        return;
    };
    let Ok((parent, mut node, mut background, mut border)) =
        hints.get_mut(show.entity)
    else {
        return;
    };
    let Ok(area) = q_area.get(parent.parent()) else {
        return;
    };
    let theme = &theme.0;

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

    let to_area = content_to_area(viewport, area);
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
