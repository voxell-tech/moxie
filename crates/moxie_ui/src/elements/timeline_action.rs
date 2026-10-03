use bevy::asset::Handle;
use bevy::color::{Alpha as _, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::Vec2;
use bevy::ui::widget::ImageNode;
use bevy::ui::{
    AlignItems, ComputedNode, Node, Overflow, UiRect, percent, px,
};
use bevy::ui_widgets::Button as ButtonBehavior;
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, SurfaceTokens as _, Tone};
use bevy_fynix::views::{Label, frame, icon, label};
use bevy_fynix::{
    AnyView, Bevy, Cx, EntityCursor, Hovered, Pressed, Prop,
    ScopedExt as _, StateExt as _, View,
};

use super::placement::Placement;
use super::timeline_block::{Selected, outline, selected_look};
use crate::drag::Dragged;
use crate::theme::EditorTheme;

/// An action icon's full size, in logical pixels.
pub const ACTION_ICON_SIZE: f32 = 14.0;
/// Gap between an action icon and its label.
const ACTION_ICON_GAP: f32 = 4.0;
/// Gap between an action icon and the subscript that continues it.
const SUBSCRIPT_HUG: f32 = 1.0;
/// Text size of the subscript.
const SUBSCRIPT_SIZE: f32 = 8.0;
/// The bar width from which the icon is at full size.
const ICON_FULL_AT: f32 = 28.0;
/// The bar width up to which the icon is gone.
const ICON_GONE_AT: f32 = 6.0;
/// Opacity of an action's name at rest.
const NAME_OPACITY: f32 = 0.9;
/// Opacity of an action's name while it is dragged.
const NAME_DRAGGED_OPACITY: f32 = 0.2;
/// Alpha of a clip's fill while it is dragged.
const DRAGGED_FILL_ALPHA: f32 = 0.2;

/// An action's field icon, tinted, with what its path adds past the
/// path the icon stands for.
pub struct ActionGlyph {
    pub image: Handle<Image>,
    pub tint: Color,
    /// Empty when the icon stands for the whole path.
    pub subscript: String,
}

/// One action's clip on the timeline: a surface-coloured, absolutely
/// placed, bordered hit area, its glyph (if any) and `name` centred
/// vertically at its left edge, in an [`ActionBody`] that fills the
/// bar. It is clipped rather than measured,
/// so a bar too narrow for them shows nothing instead of overflowing
/// its neighbour, and the icon shrinks and fades as the bar narrows
/// ([`fit_action_icons`]).
///
/// A `draft` has no subject or field yet, so it reads as an empty
/// slot in the critical colour. A `selected` clip carries
/// [`Selected`], which an app can also insert and remove later.
pub fn timeline_action(
    placement: Placement,
    name: impl Into<Prop<String>>,
    glyph: Option<ActionGlyph>,
    draft: bool,
    selected: bool,
) -> impl View<Bevy, EditorTheme> {
    let name = name.into();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        // What `fit_action_icons` settles on once laid out, so the
        // icon is never built at the wrong size.
        let fit = placement.width_px(cx.world).map_or(1.0, icon_fit);
        let fill = if draft {
            theme.color.critical.with_alpha(0.5)
        } else {
            theme.color.fill_faint
        };
        let dragged_fill = if draft {
            fill.with_alpha(DRAGGED_FILL_ALPHA)
        } else {
            Color::NONE
        };
        let border = if draft {
            theme.color.critical.with_alpha(0.5)
        } else {
            theme.color.hairline
        };
        let tone = if draft { Tone::Critical } else { Tone::Body };
        let small = theme.text.small;

        let action = cx.build(
            placement
                .apply(frame())
                .gap(0.0)
                .align(AlignItems::Center)
                .overflow(Overflow::clip())
                .radius(theme.space.radius)
                .fill(fill)
                .border_color(border)
                .when::<Hovered, _>(|frame, theme: &EditorTheme| {
                    frame.fill(theme.hover())
                })
                .when::<Pressed, _>(|frame, theme: &EditorTheme| {
                    frame.fill(theme.pressed())
                })
                .when::<Dragged, _>(move |frame, _: &EditorTheme| {
                    frame.fill(dragged_fill).border_color(
                        border.with_alpha(DRAGGED_FILL_ALPHA),
                    )
                })
                // After the dragged rule, so a selected clip keeps
                // its look while it is dragged.
                .when::<Selected, _>(selected_look),
        );
        cx.world.entity_mut(action).insert((
            ButtonBehavior,
            EntityCursor(SystemCursorIcon::Pointer),
            ActionClip,
        ));
        outline(cx.world, action, selected);
        // Takes what the bar has left, so a view built beside it
        // under the bar keeps its own width.
        let body = cx.under(action, |cx| {
            cx.build(
                frame()
                    .grow(1.0)
                    .min_width(px(0.0))
                    .height(percent(100.0))
                    .padding(UiRect::left(px(4.0)))
                    .gap(0.0)
                    .align(AlignItems::Center)
                    .overflow(Overflow::clip()),
            )
        });
        cx.world.entity_mut(body).insert(ActionBody);
        cx.under(body, |cx| {
            if let Some(glyph) = glyph {
                let hugged = !glyph.subscript.is_empty();
                let image = cx.build(
                    icon(glyph.image)
                        .size(ACTION_ICON_SIZE * fit)
                        .tint(Some(glyph.tint))
                        .opacity(fit),
                );
                let after = if hugged {
                    SUBSCRIPT_HUG
                } else {
                    ACTION_ICON_GAP
                };
                set_margin_right(cx.world, image, after);
                if hugged {
                    let sub = cx.build(
                        label(glyph.subscript)
                            .size(SUBSCRIPT_SIZE)
                            .tone(Tone::Faint)
                            .wrap(false),
                    );
                    set_margin_right(cx.world, sub, ACTION_ICON_GAP);
                }
            }
            cx.build(label(name).size(small).tone(tone).wrap(false));
        });
        action
    })
    // A rule that waits on no state sets the name's opacity, so the
    // dragged rule below can beat it.
    .rules(|cx: &mut Cx<'_, Bevy, EditorTheme>| {
        cx.set::<Label>(|label, _| label.opacity(NAME_OPACITY));
    })
    .when::<Dragged, _>(|cx: &mut Cx<'_, Bevy, EditorTheme>| {
        cx.set::<Label>(|label, _| {
            label.opacity(NAME_DRAGGED_OPACITY)
        });
    })
    .transition(Motion::Interact)
}

fn set_margin_right(world: &mut World, node: Entity, margin: f32) {
    if let Some(mut ui) = world.get_mut::<Node>(node) {
        ui.margin.right = px(margin);
    }
}

/// A [`timeline_action`]'s node, for [`fit_action_icons`] to find.
#[derive(Component)]
pub struct ActionClip;

/// The node under a [`timeline_action`] holding its glyph and name.
#[derive(Component)]
pub struct ActionBody;

/// How much of its full size an action icon keeps on a bar `width`
/// logical pixels wide: all of it on a wide bar, none on one too
/// narrow to show it.
pub fn icon_fit(width: f32) -> f32 {
    ((width - ICON_GONE_AT) / (ICON_FULL_AT - ICON_GONE_AT))
        .clamp(0.0, 1.0)
}

/// Scales every action's icon down and fades it out as its bar
/// narrows ([`icon_fit`]).
///
/// Reads the laid-out width rather than the placement, so a bar
/// being dragged narrower follows along. A clip not laid out yet has
/// no width to read: it keeps what it was built with.
pub fn fit_action_icons(
    clips: Query<
        (&ComputedNode, &bevy::ecs::hierarchy::Children),
        With<ActionClip>,
    >,
    bodies: Query<&bevy::ecs::hierarchy::Children, With<ActionBody>>,
    mut icons: Query<(&mut Node, &mut ImageNode)>,
) {
    for (computed, children) in &clips {
        if computed.size() == Vec2::ZERO {
            continue;
        }
        let fit = icon_fit(
            computed.size().x * computed.inverse_scale_factor(),
        );

        let inside = children
            .iter()
            .filter_map(|child| bodies.get(*child).ok())
            .flatten();
        for child in inside {
            let Ok((mut node, mut image)) = icons.get_mut(*child)
            else {
                continue;
            };

            let size = px(ACTION_ICON_SIZE * fit);
            if node.width != size {
                node.width = size;
                node.height = size;
            }
            if image.color.alpha() != fit {
                image.color.set_alpha(fit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::world::World;
    use bevy::time::TimePlugin;
    use bevy::ui::widget::Text;
    use bevy::ui::{BorderColor, Val};
    use bevy_fynix::{
        FynixPlugin, ReducedMotion, Theme, mount, resource,
    };

    use super::*;

    /// A clip laid out `width` wide, holding one icon; returns the
    /// icon after [`fit_action_icons`] has run.
    fn fitted(width: f32) -> (Node, f32) {
        let mut world = World::new();
        let icon =
            world.spawn((Node::default(), ImageNode::default())).id();
        let body = world.spawn(ActionBody).add_child(icon).id();
        world
            .spawn((
                ActionClip,
                ComputedNode {
                    size: Vec2::new(width, 26.0),
                    inverse_scale_factor: 1.0,
                    ..Default::default()
                },
            ))
            .add_child(body);

        world
            .run_system_cached(fit_action_icons)
            .expect("system runs");

        let node = world.get::<Node>(icon).unwrap().clone();
        let alpha =
            world.get::<ImageNode>(icon).unwrap().color.alpha();
        (node, alpha)
    }

    #[test]
    fn wide_bar_shows_the_icon_in_full() {
        let (node, alpha) = fitted(200.0);
        assert_eq!(node.width, px(ACTION_ICON_SIZE));
        assert_eq!(node.height, px(ACTION_ICON_SIZE));
        assert_eq!(alpha, 1.0);
    }

    #[test]
    fn narrowing_bar_scales_and_fades_together() {
        let (node, alpha) =
            fitted((ICON_FULL_AT + ICON_GONE_AT) / 2.0);
        assert_eq!(node.width, px(ACTION_ICON_SIZE / 2.0));
        assert_eq!(alpha, 0.5);
    }

    #[test]
    fn bar_too_narrow_for_it_leaves_nothing() {
        let (node, alpha) = fitted(2.0);
        assert_eq!(node.width, px(0.0));
        assert_eq!(alpha, 0.0);
    }

    #[test]
    fn unlaid_out_clip_keeps_its_built_size() {
        let mut world = World::new();
        let icon = world
            .spawn((
                Node {
                    width: px(ACTION_ICON_SIZE),
                    height: px(ACTION_ICON_SIZE),
                    ..Default::default()
                },
                ImageNode::default(),
            ))
            .id();
        let body = world.spawn(ActionBody).add_child(icon).id();
        world
            .spawn((ActionClip, ComputedNode::default()))
            .add_child(body);

        world
            .run_system_cached(fit_action_icons)
            .expect("system runs");

        let node = world.get::<Node>(icon).unwrap();
        assert_eq!(node.width, px(ACTION_ICON_SIZE));
        assert_eq!(
            world.get::<ImageNode>(icon).unwrap().color.alpha(),
            1.0
        );
    }

    #[test]
    fn fit_is_a_ramp_between_the_two_widths() {
        assert_eq!(icon_fit(ICON_GONE_AT), 0.0);
        assert_eq!(icon_fit(ICON_FULL_AT), 1.0);
        assert_eq!(icon_fit(0.0), 0.0);
        assert_eq!(icon_fit(1000.0), 1.0);
    }

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

    fn placed(width: f32) -> Placement {
        Placement::new(px(1.0), px(2.0), px(width), px(32.0))
    }

    fn edge(app: &App, node: Entity) -> BorderColor {
        *app.world().get::<BorderColor>(node).unwrap()
    }

    /// What the action's body holds.
    fn kids(app: &App, node: Entity) -> Vec<Entity> {
        let body = app.world().get::<Children>(node).unwrap()[0];
        app.world().get::<Children>(body).unwrap().iter().collect()
    }

    fn name_of(app: &App, node: Entity) -> Entity {
        *kids(app, node).last().unwrap()
    }

    fn mounted(app: &mut App, draft: bool, selected: bool) -> Entity {
        mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                placed(80.0),
                "move",
                None,
                draft,
                selected,
            ),
        )
    }

    fn glyph(subscript: &str) -> Option<ActionGlyph> {
        Some(ActionGlyph {
            image: Handle::default(),
            tint: Color::srgb(0.2, 0.4, 0.6),
            subscript: subscript.to_string(),
        })
    }

    #[test]
    fn the_icon_takes_its_tint_and_a_subscript_follows_it() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                placed(80.0),
                "Cube",
                glyph(".x"),
                false,
                false,
            ),
        );

        let kids = kids(&app, node);
        assert_eq!(kids.len(), 3);
        assert_eq!(
            app.world().get::<ImageNode>(kids[0]).unwrap().color,
            Color::srgb(0.2, 0.4, 0.6)
        );
        assert_eq!(app.world().get::<Text>(kids[1]).unwrap().0, ".x");
        assert_eq!(
            app.world().get::<Text>(kids[2]).unwrap().0,
            "Cube"
        );
    }

    #[test]
    fn no_subscript_leaves_only_the_icon_and_the_name() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                placed(80.0),
                "Cube",
                glyph(""),
                false,
                false,
            ),
        );

        assert_eq!(kids(&app, node).len(), 2);
    }

    #[test]
    fn an_icon_is_built_at_the_size_its_bar_fits() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                placed((ICON_FULL_AT + ICON_GONE_AT) / 2.0),
                "",
                glyph(""),
                false,
                false,
            ),
        );

        let icon = kids(&app, node)[0];
        let ui = app.world().get::<Node>(icon).unwrap();
        assert_eq!(ui.width, px(ACTION_ICON_SIZE / 2.0));
        assert_eq!(ui.height, px(ACTION_ICON_SIZE / 2.0));
        assert_eq!(
            app.world().get::<ImageNode>(icon).unwrap().color.alpha(),
            0.5
        );
    }

    #[test]
    fn bound_placement_and_name_change_the_same_nodes() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                Placement::new(
                    px(0.0),
                    px(0.0),
                    resource::<Span, _>(|span| px(span.0)),
                    px(32.0),
                ),
                resource::<Span, _>(|span| format!("{}", span.0)),
                None,
                false,
                false,
            ),
        );
        let name = name_of(&app, node);

        app.world_mut().resource_mut::<Span>().0 = 60.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, px(60.0));
        assert_eq!(ui.border, UiRect::all(px(1.0)));
        assert_eq!(name_of(&app, node), name);
        assert_eq!(app.world().get::<Text>(name).unwrap().0, "60");
    }

    #[test]
    fn selecting_recolours_the_border_and_keeps_its_width() {
        let mut app = app();
        let node = mounted(&mut app, false, false);
        app.update();
        let rest = edge(&app, node);
        let width = app.world().get::<Node>(node).unwrap().border;

        app.world_mut().entity_mut(node).insert(Selected);
        app.update();

        assert_ne!(edge(&app, node), rest);
        assert_eq!(
            app.world().get::<Node>(node).unwrap().border,
            width
        );
    }

    #[test]
    fn val_width_that_is_not_pixels_fits_in_full() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_action(
                Placement::new(px(0.0), px(0.0), Val::Auto, px(32.0)),
                "",
                glyph(""),
                false,
                false,
            ),
        );

        let icon = kids(&app, node)[0];
        let ui = app.world().get::<Node>(icon).unwrap();
        assert_eq!(ui.width, px(ACTION_ICON_SIZE));
    }
}
