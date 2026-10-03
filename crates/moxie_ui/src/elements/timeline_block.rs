use bevy::color::{Alpha as _, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Node, Overflow, UiRect, px};
use bevy_fynix::tokens::Motion;
use bevy_fynix::views::{Frame, frame};
use bevy_fynix::{AnyView, Bevy, ScopedExt as _, View, ViewSeq};

use super::placement::Placement;
use crate::drag::Dragged;
use crate::theme::EditorTheme;

/// State of a selected timeline box, for `when::<Selected, _>`
/// rules.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Selected;

const SELECTED_FILL_ALPHA: f32 = 0.12;
const SELECTED_EDGE_ALPHA: f32 = 0.5;
/// A box's border width, in logical pixels.
const BORDER: f32 = 1.0;
/// Alpha of a block's border while it is dragged.
const DRAGGED_EDGE_ALPHA: f32 = 0.04;

/// A block's box: an absolutely placed, bordered container holding
/// `children`. A selected block is tinted purple; one being dragged
/// fades its border.
pub fn timeline_block<C>(
    placement: Placement,
    selected: bool,
    children: C,
) -> impl View<Bevy, EditorTheme>
where
    C: ViewSeq<Bevy, EditorTheme> + 'static,
{
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let block = cx.build(
            placement
                .apply(frame())
                // Without this the header row stretches to the whole
                // block's height instead of sitting at its top.
                .align(AlignItems::Start)
                .overflow(Overflow::clip())
                .radius(theme.space.card_radius)
                .fill(theme.color.fill_faint)
                .border_color(theme.color.hairline)
                .when::<Selected, _>(selected_look)
                .when::<Dragged, _>(|frame, theme: &EditorTheme| {
                    frame.border_color(
                        theme
                            .color
                            .hairline
                            .with_alpha(DRAGGED_EDGE_ALPHA),
                    )
                }),
        );
        outline(cx.world, block, selected);
        cx.under(block, |cx| children.build_each(cx));
        block
    })
    .transition(Motion::Interact)
}

/// The fill of what is selected on the timeline.
pub fn selected_fill(theme: &EditorTheme) -> Color {
    theme.palette.purple.with_alpha(SELECTED_FILL_ALPHA)
}

/// The fill and border of a selected box.
pub(super) fn selected_look(
    frame: Frame,
    theme: &EditorTheme,
) -> Frame {
    frame.fill(selected_fill(theme)).border_color(
        theme.palette.purple.with_alpha(SELECTED_EDGE_ALPHA),
    )
}

/// Gives `node` its border, and [`Selected`] if `selected`.
pub(super) fn outline(
    world: &mut World,
    node: Entity,
    selected: bool,
) {
    let mut entity = world.entity_mut(node);
    if let Some(mut ui) = entity.get_mut::<Node>() {
        ui.border = UiRect::all(px(BORDER));
    }
    if selected {
        entity.insert(Selected);
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::{Node, UiRect, px};
    use bevy_fynix::{
        FynixPlugin, ReducedMotion, Theme, mount, resource,
    };

    use super::*;

    #[derive(Resource)]
    struct Span(f32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<EditorTheme>::default(),
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .insert_resource(ReducedMotion(true))
        .insert_resource(Span(100.0));
        app.update();
        app
    }

    #[test]
    fn a_bound_placement_moves_the_same_node() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_block(
                Placement::new(
                    px(0.0),
                    px(0.0),
                    resource::<Span, _>(|span| px(span.0)),
                    px(4.0),
                ),
                false,
                (),
            ),
        );

        app.world_mut().resource_mut::<Span>().0 = 250.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, px(250.0));
        assert_eq!(ui.border, UiRect::all(px(1.0)));
    }
}
