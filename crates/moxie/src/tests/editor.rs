use std::path::Path;

use bevy::asset::uuid::Uuid;
use bevy::input::keyboard::Key;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy_motiongfx::scene::id::EntityUid;
use moxie_asset::{ABSOLUTE_SOURCE, AssetRef, InternalAssets};

use super::harness::{Editor, SETTLE};
use crate::{SelectedEntity, presets, project};

/// Adds a cube as the hierarchy's add menu does, and hands it back.
///
/// STUB: goes through the menu again once the hierarchy is ported
/// in wave 3 step 2.
fn add_cube(editor: &mut Editor) -> Entity {
    crate::ui::hierarchy::spawn_mesh(editor.world(), "Cube");
    editor.step(SETTLE);
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

    let text =
        project::serialize(editor.world(), Path::new("/project"))
            .expect("it saves");
    assert!(text.contains("Transform"), "{text}");
    assert!(text.contains("Mesh3d"), "{text}");
    assert!(!text.contains("Pickable"), "{text}");
}

#[test]
fn a_field_built_later_has_the_editor_caret() {
    let mut editor = Editor::new();
    // The inspector builds its fields once something is selected.
    add_cube(&mut editor);

    // The new text input leaves the caret at Bevy's default, which
    // is the theme's text colour.
    let text = moxie_ui::theme::EditorTheme::default().color.text;
    let world = editor.world();
    let carets = world
        .query::<&bevy::text::TextCursorStyle>()
        .iter(world)
        .map(|caret| caret.color)
        .collect::<Vec<_>>();
    assert!(!carets.is_empty());
    assert!(carets.iter().all(|&color| color == text), "{carets:?}");
}

#[test]
fn the_shell_shows_the_menu_bar_and_an_empty_inspector() {
    let mut editor = Editor::new();
    editor.text("File");
    editor.text("Nothing selected");
    // Each stubbed panel is there: its tab, and its placeholder.
    assert_eq!(editor.texts("Timeline").len(), 2);
    assert_eq!(editor.texts("Action").len(), 2);
}

#[test]
fn the_inspector_follows_the_selection() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    assert!(editor.texts("Nothing selected").is_empty());

    editor.world().insert_resource(SelectedEntity(None));
    editor.step(SETTLE);
    editor.text("Nothing selected");

    editor.world().insert_resource(SelectedEntity(Some(cube)));
    editor.step(SETTLE);
    assert!(editor.texts("Nothing selected").is_empty());
}

#[test]
fn dragging_a_number_field_scrubs_its_value() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    let (x, _) = translation_x(&mut editor);

    editor.drag(x, 50.0);

    let translation = editor
        .world()
        .get::<Transform>(cube)
        .expect("placed")
        .translation;
    assert_eq!(translation, Vec3::new(0.5, 0.0, 0.0));
}

#[test]
fn a_clicked_number_field_is_typed_into_until_enter() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    let (x, text) = translation_x(&mut editor);
    let pickable = |editor: &mut Editor| {
        editor
            .world()
            .get::<bevy::picking::Pickable>(text)
            .is_some_and(|pickable| pickable.is_hoverable)
    };
    assert!(!pickable(&mut editor), "dragged, not typed into");

    editor.click_entity(x, 1);
    let focus = editor.world().resource::<InputFocus>().get();
    assert_eq!(focus, Some(text));
    assert!(pickable(&mut editor), "the text takes the pointer");

    // The pointer lands on the text now, which selects with a drag.
    editor.drag(text, 50.0);
    let moved =
        editor.world().get::<Transform>(cube).expect("placed");
    assert_eq!(moved.translation, Vec3::ZERO);

    editor.tap(KeyCode::Enter, Key::Enter);
    let focus = editor.world().resource::<InputFocus>().get();
    assert_eq!(focus, None);
    assert!(!pickable(&mut editor), "back to dragging");
}

/// The number field for the translation's x, and the text inside it.
fn translation_x(editor: &mut Editor) -> (Entity, Entity) {
    let mut row = editor.text("translation");
    let world = editor.world();
    let text = loop {
        row = world.get::<ChildOf>(row).expect("in a row").parent();
        if let Some(text) = first_number_input(world, row) {
            break text;
        }
    };
    let field =
        world.get::<ChildOf>(text).expect("in a field").parent();
    (field, text)
}

/// The text of the first number field under `root`, depth first.
/// A number field keeps its text out of the pointer's way until it
/// is typed into, which a text field does not.
fn first_number_input(world: &World, root: Entity) -> Option<Entity> {
    let children = world.get::<Children>(root)?;
    children.iter().find_map(|child| {
        let is_number_input = world
            .get::<bevy::text::EditableText>(child)
            .is_some()
            && world.get::<bevy::picking::Pickable>(child).is_some();
        if is_number_input {
            return Some(child);
        }
        first_number_input(world, child)
    })
}

#[test]
fn the_sample_project_opens() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/hello_world/hello_world.mox");
    let text = std::fs::read_to_string(&path).expect("it is there");

    let mut editor = Editor::new();
    project::open(editor.world(), &text, path);
    editor.step(SETTLE);

    let world = editor.world();
    let subjects = world.query::<&EntityUid>().iter(world).count();
    assert!(subjects > 0, "its subjects came back");
}

#[test]
fn a_moved_project_finds_the_files_beside_it() {
    let mut editor = Editor::new();
    let cube = add_cube(&mut editor);
    // Needn't exist: a handle keeps its path whether or not it loads.
    let robot = AssetRef::Path(
        "abs:///projects/intro/robot.glb#Mesh0/Primitive0"
            .to_string(),
    )
    .handle::<Mesh>(editor.world().resource::<AssetServer>());
    editor.world().entity_mut(cube).insert(Mesh3d(robot));

    let text = project::serialize(
        editor.world(),
        Path::new("/projects/intro"),
    )
    .expect("it saves");
    project::open(
        editor.world(),
        &text,
        "/moved/intro/intro.mox".into(),
    );
    editor.step(SETTLE);

    let world = editor.world();
    let mesh = world
        .query_filtered::<&Mesh3d, With<EntityUid>>()
        .single(world)
        .expect("the cube came back")
        .0
        .clone();
    assert_eq!(
        mesh.path().map(ToString::to_string).as_deref(),
        Some("abs:///moved/intro/robot.glb#Mesh0/Primitive0")
    );
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
        project::serialize(editor.world(), Path::new("/project"))
            .expect("it saves");
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
    project::open(editor.world(), &text, "/project/test.mox".into());
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
