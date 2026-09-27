use bevy::asset::uuid::Uuid;
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy_motiongfx::scene::id::EntityUid;
use moxie_asset::{ABSOLUTE_SOURCE, InternalAssets};

use super::harness::{Editor, SETTLE};
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
fn files_load_from_anywhere_on_disk() {
    let mut editor = Editor::new();
    let assets = editor.world().resource::<AssetServer>();
    assert!(assets.get_source(ABSOLUTE_SOURCE).is_ok());
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
fn a_mesh_or_material_is_never_none() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    // So the mesh field is the one thing showing "Cube".
    editor.world().entity_mut(cube).insert(Name::new("Box"));
    editor.step(SETTLE);

    editor.press("Cube");
    assert!(editor.texts("None").is_empty(), "no None for a mesh");
    editor.tap(KeyCode::Escape, Key::Escape);

    editor.press("Default");
    assert!(
        editor.texts("None").is_empty(),
        "no None for a material"
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
fn a_project_saves_only_marked_components() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    // Reflected, but the editor's own business.
    editor
        .world()
        .entity_mut(cube)
        .insert(bevy::picking::Pickable::default());

    let text = project::serialize(editor.world()).expect("it saves");
    assert!(text.contains("Transform"), "{text}");
    assert!(text.contains("Mesh3d"), "{text}");
    assert!(!text.contains("Pickable"), "{text}");
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

    let text = project::serialize(editor.world()).expect("it saves");
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

    assert_eq!(only_internal(&mut editor), (id, "Brick".to_string()));
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
