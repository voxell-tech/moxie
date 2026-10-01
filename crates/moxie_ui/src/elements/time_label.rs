use bevy::picking::Pickable;
use bevy::ui::{JustifyContent, Node, PositionType, UiRect, Val, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{frame, label};
use bevy_fynix::{AnyView, Bevy, Prop, View};

use crate::theme::EditorTheme;

/// Opacity of the reading against the axis.
const OPACITY: f32 = 0.7;

/// A time reading on the time axis, `x` pixels from the axis's left
/// edge and placed above the mark it reads for.
///
/// The leftmost reading sits flush, the rest centre over their mark.
pub fn time_label(
    x: impl Into<Prop<Val>>,
    text: impl Into<Prop<String>>,
) -> impl View<Bevy, EditorTheme> {
    let x = x.into();
    let text = text.into();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let size = cx.theme().text.small;
        let node = cx.build(
            frame()
                .position(PositionType::Absolute)
                .inset(UiRect {
                    top: px(1.0),
                    left: Val::Auto,
                    right: Val::Auto,
                    bottom: Val::Auto,
                })
                .width(px(0.0)),
        );
        cx.world.entity_mut(node).insert(Pickable::IGNORE);
        cx.effect(node, x, |world, node, x| {
            let centred = !matches!(x, Val::Px(x) if *x <= 0.0);
            if let Some(mut ui) = world.get_mut::<Node>(node) {
                ui.left = *x;
                ui.justify_content = if centred {
                    JustifyContent::Center
                } else {
                    JustifyContent::FlexStart
                };
                ui.padding = UiRect::left(if centred {
                    Val::ZERO
                } else {
                    px(3.0)
                });
            }
        });
        cx.under(node, |cx| {
            cx.build(
                label(text)
                    .size(size)
                    .wrap(false)
                    .tone(Tone::Dim)
                    .opacity(OPACITY),
            );
        });
        node
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::ecs::resource::Resource;
    use bevy::text::{FontSize, TextColor, TextFont};
    use bevy::time::TimePlugin;
    use bevy::ui::widget::Text;
    use bevy_fynix::{FynixPlugin, Theme, mount, resource};

    use super::*;

    #[derive(Resource)]
    struct Mark(f32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<EditorTheme>::default(),
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .insert_resource(Mark(40.0));
        app
    }

    fn reading(app: &App, node: bevy::ecs::entity::Entity) -> Entity {
        app.world()
            .get::<Children>(node)
            .unwrap()
            .iter()
            .next()
            .unwrap()
    }

    use bevy::ecs::entity::Entity;

    #[test]
    fn the_fixed_look_is_written() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            time_label(px(40.0), "0:02"),
        );

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.position_type, PositionType::Absolute);
        assert_eq!(ui.top, px(1.0));
        assert_eq!(ui.width, px(0.0));
        assert_eq!(
            app.world().get::<Pickable>(node),
            Some(&Pickable::IGNORE)
        );

        let theme = EditorTheme::default();
        let text = reading(&app, node);
        assert_eq!(app.world().get::<Text>(text).unwrap().0, "0:02");
        assert_eq!(
            app.world().get::<TextFont>(text).unwrap().font_size,
            FontSize::Px(theme.text.small)
        );
        assert_eq!(
            app.world().get::<TextColor>(text).unwrap().0,
            theme
                .color
                .text_dim
                .with_alpha(theme.color.text_dim.alpha() * OPACITY)
        );
    }

    use bevy::color::Alpha;

    #[test]
    fn a_bound_x_moves_the_same_node_and_recentres_it() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            time_label(
                resource::<Mark, _>(|mark| px(mark.0)),
                "0:02",
            ),
        );
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.left, px(40.0));
        assert_eq!(ui.justify_content, JustifyContent::Center);
        assert_eq!(ui.padding, UiRect::ZERO);

        app.world_mut().resource_mut::<Mark>().0 = 0.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.left, px(0.0));
        assert_eq!(ui.justify_content, JustifyContent::FlexStart);
        assert_eq!(ui.padding, UiRect::left(px(3.0)));
    }

    #[test]
    fn a_bound_text_changes_the_reading_in_place() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            time_label(
                px(10.0),
                resource::<Mark, _>(|mark| format!("{}", mark.0)),
            ),
        );
        let text = reading(&app, node);

        app.world_mut().resource_mut::<Mark>().0 = 7.0;
        app.update();

        assert_eq!(reading(&app, node), text);
        assert_eq!(app.world().get::<Text>(text).unwrap().0, "7");
    }
}
