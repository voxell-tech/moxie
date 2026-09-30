//! The inspector's asset field button: an icon and the name of what
//! is held, in a full-width button.

use bevy::asset::Handle;
use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::ui::{JustifyContent, percent};

use crate::prop::Signal;
use crate::tokens::{SpacingTokens, SurfaceTokens, TextTokens, Tone};
use crate::views::{BehaviorExt, button, icon, label, row};
use crate::{Bevy, View};

/// A dimmed button showing `image` and the text `name` follows,
/// running `open` when activated.
pub fn asset_field_button<T>(
    image: Handle<Image>,
    name: Signal<String>,
    open: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View<Bevy, T>
where
    T: TextTokens
        + SurfaceTokens
        + SpacingTokens
        + Send
        + Sync
        + 'static,
{
    button(
        row((icon(image), label(name).wrap(false)))
            .width(percent(100.0))
            .justify(JustifyContent::SpaceBetween),
    )
    .fill(Color::NONE)
    .width(percent(100.0))
    .toned(Tone::Dim)
    .on_activate(open)
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::resource::Resource;
    use bevy::text::{LineBreak, TextLayout};
    use bevy::ui::Val;
    use bevy::ui::widget::ImageNode;
    use bevy::ui_widgets::{Activate, Button as ButtonBehavior};

    use super::*;
    use crate::demo::testing::{
        DIM, Demo, app, color, kids, text, ui,
    };
    use crate::{mount, resource};

    #[derive(Resource)]
    struct Held(String);

    #[derive(Resource, Default)]
    struct Opened(u32);

    fn build(app: &mut App) -> Entity {
        app.insert_resource(Held("(none)".into()))
            .init_resource::<Opened>();
        mount::<Demo>(
            app.world_mut(),
            asset_field_button::<Demo>(
                Handle::default(),
                resource::<Held, _>(|held| held.0.clone()),
                |world| world.resource_mut::<Opened>().0 += 1,
            ),
        )
    }

    #[test]
    fn it_is_a_full_width_button_of_an_icon_and_a_label() {
        let mut app = app();
        let button = build(&mut app);

        assert!(app.world().get::<ButtonBehavior>(button).is_some());
        assert_eq!(ui(&app, button).width, Val::Percent(100.0));
        let [inner] = kids(&app, button)[..] else {
            panic!("one row");
        };
        assert_eq!(
            ui(&app, inner).justify_content,
            JustifyContent::SpaceBetween
        );
        let [image, name] = kids(&app, inner)[..] else {
            panic!("an icon and a label");
        };
        assert!(app.world().get::<ImageNode>(image).is_some());
        assert_eq!(text(&app, name), "(none)");
    }

    #[test]
    fn both_parts_are_dim_and_the_label_does_not_wrap() {
        let mut app = app();
        let button = build(&mut app);
        let inner = kids(&app, button)[0];
        let [image, name] = kids(&app, inner)[..] else {
            panic!("an icon and a label");
        };

        assert_eq!(color(&app, name), DIM);
        assert_eq!(
            app.world().get::<ImageNode>(image).unwrap().color,
            DIM
        );
        assert_eq!(
            app.world().get::<TextLayout>(name).unwrap().linebreak,
            LineBreak::NoWrap
        );
    }

    #[test]
    fn the_label_follows_the_world() {
        let mut app = app();
        let button = build(&mut app);
        let inner = kids(&app, button)[0];
        let name = kids(&app, inner)[1];

        app.world_mut().resource_mut::<Held>().0 = "hero.glb".into();
        app.update();

        assert_eq!(text(&app, name), "hero.glb");
    }

    #[test]
    fn activating_runs_the_handler_each_time() {
        let mut app = app();
        let button = build(&mut app);

        app.world_mut().trigger(Activate { entity: button });
        app.world_mut().trigger(Activate { entity: button });
        app.update();

        assert_eq!(app.world().resource::<Opened>().0, 2);
    }
}
