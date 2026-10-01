use core::f32::consts::FRAC_1_SQRT_2;

use bevy::math::Rot2;
use bevy::picking::Pickable;
use bevy::ui::{
    Overflow, PositionType, UiRect, UiTransform, Val, ZIndex, px,
};
use bevy_fynix::views::{FrameProps as _, row};
use bevy_fynix::{AnyView, Bevy, Prop, View};

use crate::theme::EditorTheme;

const LINE_WIDTH: f32 = 2.0;
/// The head's square before it is turned.
const HEAD_SIDE: f32 = 12.0;
/// Half the turned square's diagonal: how far it spreads and hangs.
const HEAD_REACH: f32 = HEAD_SIDE * FRAC_1_SQRT_2;

/// The playhead: a line from `top` to the bottom of its parent,
/// `left` pixels from the parent's left edge, with a head hanging
/// above it.
pub fn playhead_line(
    left: impl Into<Prop<Val>>,
    top: Val,
) -> impl View<Bevy, EditorTheme> {
    let left = left.into();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let color = cx.theme().palette.orange;
        let head = row((row(())
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(HEAD_REACH - HEAD_SIDE / 2.0),
                top: px(-HEAD_SIDE / 2.0),
                right: Val::Auto,
                bottom: Val::Auto,
            })
            .width(px(HEAD_SIDE))
            .height(px(HEAD_SIDE))
            .fill(color)
            .with((
                UiTransform::from_rotation(Rot2::degrees(45.0)),
                Pickable::IGNORE,
            )),))
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(LINE_WIDTH / 2.0 - HEAD_REACH),
            top: px(-HEAD_REACH),
            right: Val::Auto,
            bottom: Val::Auto,
        })
        .width(px(HEAD_REACH * 2.0))
        .height(px(HEAD_REACH))
        .overflow(Overflow::clip())
        .with(Pickable::IGNORE);

        let line = row((head,))
            .position(PositionType::Absolute)
            .inset(left.map(move |left| UiRect {
                left,
                top,
                bottom: px(0.0),
                right: Val::Auto,
            }))
            .width(px(LINE_WIDTH))
            .fill(color)
            .with((ZIndex(10), Pickable::IGNORE));
        cx.build(line)
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::hierarchy::Children;
    use bevy::ecs::relationship::RelationshipTarget;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::{BackgroundColor, Node};
    use bevy_fynix::{FynixPlugin, Theme, mount, resource};

    use super::*;

    #[derive(Resource)]
    struct Time(f32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<EditorTheme>::default(),
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .insert_resource(Time(10.0));
        app
    }

    #[test]
    fn the_fixed_look_is_written() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            playhead_line(px(10.0), px(20.0)),
        );

        let orange = EditorTheme::default().palette.orange;
        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.position_type, PositionType::Absolute);
        assert_eq!(
            (ui.left, ui.top, ui.bottom),
            (px(10.0), px(20.0), px(0.0))
        );
        assert_eq!(ui.width, px(LINE_WIDTH));
        assert_eq!(
            app.world().get::<BackgroundColor>(node).unwrap().0,
            orange
        );
        assert_eq!(
            app.world().get::<ZIndex>(node),
            Some(&ZIndex(10))
        );
        assert_eq!(
            app.world().get::<Pickable>(node),
            Some(&Pickable::IGNORE)
        );

        let head = app
            .world()
            .get::<Children>(node)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let ui = app.world().get::<Node>(head).unwrap();
        assert_eq!(ui.overflow, Overflow::clip());
        assert_eq!(ui.height, px(HEAD_REACH));

        let diamond = app
            .world()
            .get::<Children>(head)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert_eq!(
            app.world().get::<UiTransform>(diamond).unwrap().rotation,
            Rot2::degrees(45.0)
        );
        assert_eq!(
            app.world().get::<BackgroundColor>(diamond).unwrap().0,
            orange
        );
    }

    #[test]
    fn a_bound_left_moves_the_same_node_and_keeps_its_top() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            playhead_line(
                resource::<Time, _>(|time| px(time.0)),
                px(20.0),
            ),
        );

        app.world_mut().resource_mut::<Time>().0 = 55.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.left, px(55.0));
        assert_eq!(ui.top, px(20.0));
        assert_eq!(ui.bottom, px(0.0));
    }
}
