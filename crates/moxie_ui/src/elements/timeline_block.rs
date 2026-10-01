use bevy::color::{Alpha as _, Luminance as _};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Node, Overflow, UiRect, px};
use bevy_fynix::tokens::Motion;
use bevy_fynix::views::frame;
use bevy_fynix::{AnyView, Bevy, ScopedExt as _, View, ViewSeq};

use super::placement::Placement;
use crate::drag::Dragged;
use crate::theme::EditorTheme;

/// State of a selected timeline box, for `when::<Selected, _>`
/// rules.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Selected;

/// A block's box: an absolutely placed, bordered container holding
/// `children`. A selected block thickens its border and turns purple;
/// one being dragged fades its border.
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
        let text = theme.color.text;
        let purple = theme.palette.purple;
        let block = cx.build(
            placement
                .apply(frame())
                // Without this the header row stretches to the whole
                // block's height instead of sitting at its top.
                .align(AlignItems::Start)
                .overflow(Overflow::clip())
                .radius(theme.space.card_radius)
                .fill(text.with_alpha(0.03))
                .border_color(text.with_alpha(0.5))
                .when::<Selected, _>(move |frame, _: &EditorTheme| {
                    frame
                        .fill(
                            purple
                                .with_luminance(0.3)
                                .with_alpha(0.8),
                        )
                        .border_color(purple.with_alpha(0.5))
                })
                .when::<Dragged, _>(move |frame, _: &EditorTheme| {
                    frame.border_color(text.with_alpha(0.2))
                }),
        );
        snap_selected_border(cx.world, block, selected);
        cx.under(block, |cx| children.build_each(cx));
        block
    })
    .transition(Motion::Interact)
}

/// A selected box's border width, in logical pixels.
pub(super) const SELECTED_BORDER: f32 = 3.0;
/// An unselected box's border width.
pub(super) const REST_BORDER: f32 = 1.0;

/// Gives `node` a border that is [`SELECTED_BORDER`] wide while it
/// holds [`Selected`] and [`REST_BORDER`] otherwise, and inserts
/// `Selected` if `selected`.
///
/// The width is written straight to the node rather than through a
/// frame's `border` prop: a transition blends every prop that can
/// blend, so a width set by a rule would ease between the two.
pub(super) fn snap_selected_border(
    world: &mut World,
    node: Entity,
    selected: bool,
) {
    let mut entity = world.entity_mut(node);
    if let Some(mut ui) = entity.get_mut::<Node>() {
        ui.border = UiRect::all(px(REST_BORDER));
    }
    entity
        .observe(
            |add: On<Add, Selected>, mut nodes: Query<&mut Node>| {
                if let Ok(mut ui) = nodes.get_mut(add.entity) {
                    ui.border = UiRect::all(px(SELECTED_BORDER));
                }
            },
        )
        .observe(
            |remove: On<Remove, Selected>,
             mut nodes: Query<&mut Node>| {
                if let Ok(mut ui) = nodes.get_mut(remove.entity) {
                    ui.border = UiRect::all(px(REST_BORDER));
                }
            },
        );
    if selected {
        entity.insert(Selected);
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::color::Color;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::{
        BackgroundColor, BorderColor, BorderRadius, Node,
        PositionType, UiRect, px,
    };
    use bevy_fynix::views::label;
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

    fn placed() -> Placement {
        Placement::new(px(1.0), px(2.0), px(3.0), px(4.0))
    }

    fn fill(app: &App, node: Entity) -> Color {
        app.world().get::<BackgroundColor>(node).unwrap().0
    }

    fn edge(app: &App, node: Entity) -> BorderColor {
        *app.world().get::<BorderColor>(node).unwrap()
    }

    #[test]
    fn the_fixed_look_is_written() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_block(placed(), false, (label("kid"),)),
        );

        let theme = EditorTheme::default();
        let text = theme.color.text;
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.position_type, PositionType::Absolute);
        assert_eq!(ui.align_items, AlignItems::Start);
        assert_eq!(ui.overflow, Overflow::clip());
        assert_eq!((ui.left, ui.top), (px(1.0), px(2.0)));
        assert_eq!((ui.width, ui.height), (px(3.0), px(4.0)));
        assert_eq!(ui.border, UiRect::all(px(1.0)));
        assert_eq!(
            ui.border_radius,
            BorderRadius::all(px(theme.space.card_radius))
        );
        assert_eq!(fill(&app, node), text.with_alpha(0.03));
        assert_eq!(
            edge(&app, node),
            BorderColor::all(text.with_alpha(0.5))
        );
        assert_eq!(
            app.world().get::<Children>(node).unwrap().len(),
            1
        );
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

    #[test]
    fn selected_thickens_the_border_and_turns_purple() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_block(placed(), true, ()),
        );
        app.update();

        let purple = EditorTheme::default().palette.purple;
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.border, UiRect::all(px(3.0)));
        assert_eq!(
            fill(&app, node),
            purple.with_luminance(0.3).with_alpha(0.8)
        );
        assert_eq!(
            edge(&app, node),
            BorderColor::all(purple.with_alpha(0.5))
        );

        app.world_mut().entity_mut(node).remove::<Selected>();
        app.update();
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.border, UiRect::all(px(1.0)));
    }

    #[test]
    fn the_border_width_snaps_where_the_fill_eases() {
        let mut app = app();
        app.insert_resource(ReducedMotion(false));
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_block(placed(), false, ()),
        );
        app.update();

        app.world_mut().entity_mut(node).insert(Selected);
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.border, UiRect::all(px(3.0)));
        let target = EditorTheme::default()
            .palette
            .purple
            .with_luminance(0.3)
            .with_alpha(0.8);
        assert_ne!(fill(&app, node), target);
    }

    #[test]
    fn dragging_fades_the_border() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_block(placed(), false, ()),
        );

        app.world_mut().entity_mut(node).insert(Dragged);
        app.update();

        let text = EditorTheme::default().color.text;
        assert_eq!(
            edge(&app, node),
            BorderColor::all(text.with_alpha(0.2))
        );
    }
}
