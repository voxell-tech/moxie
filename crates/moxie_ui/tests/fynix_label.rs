//! A label built through the ECS, and a bound one patched in place.

use bevy::prelude::*;
use bevy::time::TimePlugin;
use bevy_fynix::views::label;
use bevy_fynix::{mount, resource};
use moxie_ui::MoxieUiPlugin;
use moxie_ui::theme::EditorTheme;

/// The label's text, for a bound prop to fire on.
#[derive(Resource, Default)]
struct Caption(String);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((TimePlugin, MoxieUiPlugin))
        .init_resource::<Caption>();
    app
}

#[test]
fn label_writes_its_props_as_components() {
    let mut app = app();
    let node = mount::<EditorTheme>(
        app.world_mut(),
        label("Save").size(20.0),
    );
    app.update();

    let world = app.world();
    assert_eq!(world.get::<Text>(node).unwrap().0, "Save");
    assert_eq!(
        world.get::<TextFont>(node).unwrap().font_size,
        FontSize::Px(20.0)
    );
}

#[test]
fn bound_prop_is_patched_without_a_rebuild() {
    let mut app = app();
    let node = mount::<EditorTheme>(
        app.world_mut(),
        label(resource::<Caption, _>(|caption| caption.0.clone())),
    );
    app.update();

    app.world_mut().resource_mut::<Caption>().0 = "Saved".into();
    app.update();

    assert_eq!(app.world().get::<Text>(node).unwrap().0, "Saved");
}
