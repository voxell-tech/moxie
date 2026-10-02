use bevy::asset::Handle;
use bevy::color::Color;
use bevy::image::Image;
use bevy::picking::Pickable;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy_fynix::patch::Paint;
use bevy_fynix::views::frame;
use bevy_fynix::{AnyView, Bevy, View};

use super::placement::Placement;
use crate::theme::EditorTheme;

/// The span before a node's own delay ends: `image` tiled in `color`
/// over where the node would have started. It ignores the pointer, so
/// a click on it reaches the track under it.
pub fn timeline_gap(
    placement: Placement,
    image: Handle<Image>,
    color: Color,
) -> impl View<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let gap = cx.build(placement.apply(frame()));
        let mut gap = cx.world.entity_mut(gap);
        // The paint keeps the tint, so a later repaint of the frame
        // writes it back rather than clearing it.
        if let Some(mut paint) = gap.get_mut::<Paint>() {
            paint.ink = color;
        }
        gap.insert((
            ImageNode {
                image,
                color,
                image_mode: NodeImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..ImageNode::default()
            },
            Pickable::IGNORE,
        ));
        gap.id()
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::{Node, px};
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
        .insert_resource(Span(10.0));
        app
    }

    #[test]
    fn a_bound_width_resizes_the_same_node_and_keeps_the_tint() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_gap(
                Placement::new(
                    px(0.0),
                    px(0.0),
                    resource::<Span, _>(|span| px(span.0)),
                    px(4.0),
                ),
                Handle::default(),
                Color::WHITE,
            ),
        );

        app.world_mut().resource_mut::<Span>().0 = 40.0;
        app.update();

        assert_eq!(
            app.world().get::<Node>(node).unwrap().width,
            px(40.0)
        );
        assert_eq!(
            app.world().get::<ImageNode>(node).unwrap().color,
            Color::WHITE
        );
    }
}
