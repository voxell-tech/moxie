//! The whole editor, headless, for tests: no window, no GPU, and frames
//! stepped by hand.
//!
//! With no window nothing is laid out, so nothing is found by where it
//! is drawn. A test finds a control by the text it shows, or a
//! tooltip's name, and acts on it the way a click or a key would.

use core::time::Duration;

use bevy::asset::UnapprovedPathMode;
use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::log::LogPlugin;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Click, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::sync_world::SyncWorldPlugin;
use bevy::ui_widgets::{
    Activate, Button as ButtonBehavior, MenuItem,
};
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use moxie_asset::register_absolute_source;

use crate::MoxiePlugin;

/// Frames an interaction is given to settle: its commands to apply,
/// and the UI it changes to rebuild.
const SETTLE: usize = 3;

pub(crate) struct Editor {
    pub(crate) app: App,
}

impl Editor {
    pub(crate) fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            register_absolute_source,
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: "../../assets".into(),
                    unapproved_path_mode: UnapprovedPathMode::Deny,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(
                        Box::new(WgpuSettings {
                            backends: None,
                            ..default()
                        }),
                    ),
                    ..default()
                })
                .disable::<WinitPlugin>()
                // Tests run side by side, and only one of them could own
                // the global logger.
                .disable::<LogPlugin>(),
            // Rendering with no GPU never adds this, but the hooks on
            // anything drawable still reach for what it keeps.
            SyncWorldPlugin,
            MoxiePlugin,
        ));
        app.finish();
        app.cleanup();

        let mut editor = Self { app };
        editor.step(SETTLE);
        editor
    }

    /// Runs `frames` frames.
    pub(crate) fn step(&mut self, frames: usize) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    pub(crate) fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Every visible entity showing exactly `text`. A shut menu's rows
    /// are there but hidden, and don't count.
    pub(crate) fn texts(&mut self, text: &str) -> Vec<Entity> {
        let world = self.world();
        world
            .query::<(Entity, &Text, &InheritedVisibility)>()
            .iter(world)
            .filter(|(_, shown, visible)| {
                shown.0 == text && visible.get()
            })
            .map(|(entity, ..)| entity)
            .collect()
    }

    /// The one entity showing `text`.
    pub(crate) fn text(&mut self, text: &str) -> Entity {
        match self.texts(text).as_slice() {
            [entity] => *entity,
            found => panic!(
                "{} entities show {text:?}, not one",
                found.len()
            ),
        }
    }

    /// The one UI node named `name`, the way a tooltip names its
    /// button.
    pub(crate) fn named(&mut self, name: &str) -> Entity {
        let world = self.world();
        let found = world
            .query_filtered::<(Entity, &Name), With<Node>>()
            .iter(world)
            .filter(|(_, named)| named.as_str() == name)
            .map(|(entity, _)| entity)
            .collect::<Vec<_>>();
        match found.as_slice() {
            [entity] => *entity,
            found => panic!(
                "{} nodes are named {name:?}, not one",
                found.len()
            ),
        }
    }

    /// Presses the button or menu row showing `text`.
    pub(crate) fn press(&mut self, text: &str) {
        let label = self.text(text);
        self.press_entity(label);
    }

    /// Presses the button or menu row `entity` sits in, as a click or
    /// Enter on it would.
    pub(crate) fn press_entity(&mut self, entity: Entity) {
        let world = self.world();
        let mut at = entity;
        while world.get::<ButtonBehavior>(at).is_none()
            && world.get::<MenuItem>(at).is_none()
        {
            at = world
                .get::<ChildOf>(at)
                .unwrap_or_else(|| panic!("{entity} is in no button"))
                .parent();
        }
        world.trigger(Activate { entity: at });
        self.step(SETTLE);
    }

    /// Clicks the entity showing `text`, `count` times in a row. The
    /// click bubbles up from there, as a real one does.
    pub(crate) fn click(&mut self, text: &str, count: u8) {
        let entity = self.text(text);
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        let click = Click {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: Duration::ZERO,
            count,
        };
        self.world().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            click,
            entity,
        ));
        self.step(SETTLE);
    }

    /// Taps `key`: pressed for a frame, then let go.
    pub(crate) fn tap(&mut self, key: KeyCode, logical: Key) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.world().write_message(KeyboardInput {
                key_code: key,
                logical_key: logical.clone(),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            self.step(1);
        }
        self.step(SETTLE);
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::uuid::Uuid;
    use bevy_motiongfx::scene::id::EntityUid;
    use moxie_asset::InternalAssets;

    use super::*;
    use crate::{SelectedEntity, presets, project};

    /// Adds a cube from the hierarchy's add menu, and hands it back.
    fn add_cube(editor: &mut Editor) -> Entity {
        let add = editor.named("Add");
        editor.press_entity(add);
        editor.press("Cube");
        editor
            .world()
            .resource::<SelectedEntity>()
            .0
            .expect("the new cube is selected")
    }

    fn material_of(
        editor: &mut Editor,
        entity: Entity,
    ) -> Handle<StandardMaterial> {
        editor
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .expect("it has a material")
            .0
            .clone()
    }

    /// The one internal asset, by id and name.
    fn only_internal(editor: &mut Editor) -> (Uuid, String) {
        let internal = editor.world().resource::<InternalAssets>();
        let all = internal.iter().collect::<Vec<_>>();
        let [asset] = all.as_slice() else {
            panic!("{} internal assets, not one", all.len());
        };
        (asset.id, asset.name.clone())
    }

    #[test]
    fn add_menu_makes_a_visible_cube() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);

        let world = editor.world();
        assert_eq!(
            world.get::<Name>(cube).map(Name::as_str),
            Some("Cube")
        );
        assert!(world.get::<EntityUid>(cube).is_some());
        assert!(world.get::<Mesh3d>(cube).is_some());
        assert_eq!(
            material_of(&mut editor, cube).id(),
            presets::DEFAULT_MATERIAL.id(),
        );
    }

    #[test]
    fn new_makes_an_internal_material() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);

        // The material field, showing the built-in it holds.
        editor.press("Default");
        editor.press("New");

        let (id, name) = only_internal(&mut editor);
        assert_eq!(name, "Material");
        assert_eq!(material_of(&mut editor, cube), Handle::from(id));
    }

    #[test]
    fn escape_puts_the_material_back() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);
        editor.press("Default");
        editor.press("New");
        editor.tap(KeyCode::Enter, Key::Enter);
        let (id, _) = only_internal(&mut editor);
        editor
            .world()
            .resource_mut::<InternalAssets>()
            .rename(id, "Brick".to_string());
        editor.step(SETTLE);

        editor.press("Brick");
        editor.click("Default", 1);
        assert_eq!(
            material_of(&mut editor, cube).id(),
            presets::DEFAULT_MATERIAL.id(),
            "a click assigns at once",
        );
        editor.tap(KeyCode::Escape, Key::Escape);
        assert_eq!(material_of(&mut editor, cube), Handle::from(id));
    }

    #[test]
    fn a_mesh_is_never_none() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);
        // So the mesh field is the one thing showing "Cube".
        editor.world().entity_mut(cube).insert(Name::new("Box"));
        editor.step(SETTLE);

        editor.press("Cube");
        assert!(
            editor.texts("None").is_empty(),
            "no None for a mesh"
        );
        editor.tap(KeyCode::Escape, Key::Escape);

        editor.press("Default");
        assert_eq!(
            editor.texts("None").len(),
            1,
            "None for a material"
        );
    }

    #[test]
    fn double_click_picks_and_closes() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);
        editor.press("Default");
        editor.press("New");

        editor.click("Default", 2);
        assert_eq!(
            material_of(&mut editor, cube).id(),
            presets::DEFAULT_MATERIAL.id(),
        );
        assert!(editor.texts("New").is_empty(), "the picker closed");
    }

    #[test]
    fn project_keeps_internal_materials() {
        let mut editor = Editor::new();
        let cube = add_cube(&mut editor);
        editor.press("Default");
        editor.press("New");
        editor.tap(KeyCode::Enter, Key::Enter);
        let (id, _) = only_internal(&mut editor);
        editor
            .world()
            .resource_mut::<InternalAssets>()
            .rename(id, "Brick".to_string());
        let red = Color::srgb(1.0, 0.0, 0.0);
        let material = material_of(&mut editor, cube).id();
        editor
            .world()
            .resource_mut::<Assets<StandardMaterial>>()
            .get_mut(material)
            .expect("the material is kept")
            .base_color = red;

        let text =
            project::serialize(editor.world()).expect("it saves");
        project::new_scene(editor.world());
        editor.step(SETTLE);
        assert!(
            editor
                .world()
                .resource::<InternalAssets>()
                .iter()
                .next()
                .is_none()
        );
        project::open(editor.world(), &text, "test.mox".into());
        editor.step(SETTLE);

        assert_eq!(
            only_internal(&mut editor),
            (id, "Brick".to_string())
        );
        let world = editor.world();
        let cube = world
            .query_filtered::<(Entity, &Name), With<EntityUid>>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "Cube")
            .map(|(entity, _)| entity)
            .expect("the cube came back");
        let material = material_of(&mut editor, cube);
        assert_eq!(material, Handle::from(id));
        let base_color = editor
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(material.id())
            .expect("its material came back")
            .base_color;
        assert_eq!(base_color, red);
    }
}
