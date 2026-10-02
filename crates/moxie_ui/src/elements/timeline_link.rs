use bevy::color::Color;
use bevy::picking::Pickable;
use bevy::ui::{Node, UiRect, px};
use bevy_fynix::views::frame;
use bevy_fynix::{AnyView, Bevy, Prop, View};

use super::placement::Placement;
use crate::theme::EditorTheme;

/// A right-angle line joining one flow child's start to the next:
/// down its left edge, then along its bottom.
pub fn timeline_link(
    placement: Placement,
    color: impl Into<Prop<Color>>,
) -> impl View<Bevy, EditorTheme> {
    let color = color.into();
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let link =
            cx.build(placement.apply(frame()).border_color(color));
        let mut link = cx.world.entity_mut(link);
        if let Some(mut ui) = link.get_mut::<Node>() {
            ui.border = UiRect {
                left: px(3.0),
                bottom: px(3.0),
                ..UiRect::ZERO
            };
        }
        link.insert(Pickable::IGNORE);
        link.id()
    })
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::resource::Resource;
    use bevy::time::TimePlugin;
    use bevy::ui::BorderColor;
    use bevy_fynix::{FynixPlugin, Theme, mount, resource};

    use super::*;

    #[derive(Resource)]
    struct Tint(Color);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixPlugin::<EditorTheme>::default(),
        ))
        .insert_resource(Theme(EditorTheme::default()))
        .insert_resource(Tint(Color::WHITE));
        app
    }

    #[test]
    fn bound_placement_and_colour_move_the_same_node() {
        let mut app = app();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            timeline_link(
                Placement::new(
                    resource::<Tint, _>(|tint| {
                        px(tint.0.to_srgba().red * 10.0)
                    }),
                    px(2.0),
                    resource::<Tint, _>(|tint| {
                        px(tint.0.to_srgba().red * 20.0)
                    }),
                    px(4.0),
                ),
                resource::<Tint, _>(|tint| tint.0),
            ),
        );

        app.world_mut().resource_mut::<Tint>().0 =
            Color::srgb(0.5, 0.0, 0.0);
        app.update();

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.left, px(5.0));
        assert_eq!(ui.width, px(10.0));
        assert_eq!(ui.border.left, px(3.0), "border survives");
        assert_eq!(
            app.world().get::<BorderColor>(node),
            Some(&BorderColor::all(Color::srgb(0.5, 0.0, 0.0)))
        );
    }
}
