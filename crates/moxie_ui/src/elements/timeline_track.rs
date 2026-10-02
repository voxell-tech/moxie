use bevy::ui::{Node, Val, percent};
use bevy_fynix::views::frame;
use bevy_fynix::{AnyView, Bevy, Prop, View, ViewSeq};

use crate::theme::EditorTheme;

/// The scrubbable timeline track: a node as wide as the track's
/// duration, holding `children`. The app resolves its pixels per
/// second and passes the result as `width`, so a clip at time `t`
/// sits `t * pixels_per_second` from the track's left edge.
pub fn timeline_track<C>(
    width: impl Into<Prop<Val>>,
    children: C,
) -> impl View<Bevy, EditorTheme>
where
    C: ViewSeq<Bevy, EditorTheme> + 'static,
{
    let width = width.into();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let track = cx.build(frame().height(percent(100.0)));
        // The minimum follows the width, or a flex parent would
        // shrink the track below its duration.
        cx.effect(track, width, |world, track, width| {
            if let Some(mut ui) = world.get_mut::<Node>(track) {
                ui.width = *width;
                ui.min_width = *width;
            }
        });
        cx.under(track, |cx| children.build_each(cx));
        track
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::px;
    use bevy_fynix::{FynixPlugin, Theme, mount, resource};

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
        .insert_resource(Span(100.0));
        app
    }

    #[test]
    fn a_bound_width_resizes_the_same_node_and_its_minimum() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_track(
                resource::<Span, _>(|span| px(span.0)),
                (),
            ),
        );
        assert_eq!(
            app.world().get::<Node>(node).unwrap().width,
            px(100.0)
        );

        app.world_mut().resource_mut::<Span>().0 = 250.0;
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, px(250.0));
        assert_eq!(ui.min_width, px(250.0));
    }
}
