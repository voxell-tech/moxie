use bevy::color::Color;
use bevy::picking::Pickable;
use bevy::ui::{PositionType, UiRect, Val, px};
use bevy_fynix::views::{BehaviorExt as _, frame};
use bevy_fynix::{Bevy, Prop, View};

use crate::theme::EditorTheme;

/// One mark on the time axis, `x` pixels from the axis's left edge.
/// It grows upward from the axis's bottom edge, so marks of
/// different `height` share a baseline.
pub fn time_tick(
    x: impl Into<Prop<Val>>,
    height: impl Into<Prop<Val>>,
    color: impl Into<Prop<Color>>,
) -> impl View<Bevy, EditorTheme> {
    frame()
        .position(PositionType::Absolute)
        .inset(x.into().map(|left| UiRect {
            left,
            bottom: px(0.0),
            top: Val::Auto,
            right: Val::Auto,
        }))
        .width(px(1.0))
        .height(height)
        .fill(color)
        .tagged(Pickable::IGNORE)
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::{BackgroundColor, Node};
    use bevy_fynix::{FynixPlugin, Theme, mount, resource};

    use super::*;

    #[derive(Resource)]
    struct Wide(f32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<EditorTheme>::default(),
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .insert_resource(Wide(10.0));
        app
    }

    #[test]
    fn a_bound_x_moves_the_same_node() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            time_tick(
                resource::<Wide, _>(|wide| px(wide.0)),
                px(4.0),
                Color::WHITE,
            ),
        );
        assert_eq!(
            app.world().get::<Node>(node).unwrap().left,
            px(10.0)
        );

        app.world_mut().resource_mut::<Wide>().0 = 30.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.left, px(30.0));
        assert_eq!(ui.bottom, px(0.0));
    }

    #[test]
    fn a_bound_height_and_color_follow_too() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            time_tick(
                px(0.0),
                resource::<Wide, _>(|wide| px(wide.0)),
                resource::<Wide, _>(|wide| {
                    Color::srgb(wide.0, 0.0, 0.0)
                }),
            ),
        );

        app.world_mut().resource_mut::<Wide>().0 = 1.0;
        app.update();

        assert_eq!(
            app.world().get::<Node>(node).unwrap().height,
            px(1.0)
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(node).unwrap().0,
            Color::srgb(1.0, 0.0, 0.0)
        );
    }
}
