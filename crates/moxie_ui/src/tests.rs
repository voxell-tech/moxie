//! Shared test helpers: a headless app with the plugin in it, the
//! reflected types the inspector tests edit, and node lookups.

use core::time::Duration;

use bevy::app::{App, TaskPoolPlugin};
use bevy::asset::{AssetPlugin, UnapprovedPathMode};
use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::hierarchy::Children;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::{ButtonState, InputPlugin};
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::picking::backend::HitData;
use bevy::picking::events::{
    Click, Drag, DragDrop, DragEnd, DragStart, Pointer, Press,
};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy::ui::UiScale;
use bevy::ui_widgets::{MenuAction, MenuButton, MenuEvent};
use bevy::window::PrimaryWindow;
use bevy_fynix::views::{button, ghost, label, row};
use bevy_fynix::{Bevy, ScopedExt as _, View, mount};

use crate::MoxieUiPlugin;
use crate::theme::EditorTheme;

/// A headless app under the editor theme, whose clock moves 50ms per
/// update.
pub fn app() -> App {
    let mut app = App::new();
    // Before `AssetPlugin`, which builds the sources.
    moxie_asset::register_absolute_source(&mut app);
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        // As the editor sets it, for a file from anywhere on disk.
        AssetPlugin {
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..AssetPlugin::default()
        },
        InputPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        MoxieUiPlugin,
    ))
    .init_asset::<Image>()
    .init_resource::<UiScale>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(50),
    ));
    app.world_mut().spawn(PrimaryWindow);
    // The first update only starts the clock.
    app.update();
    app
}

/// Mounts `view` and runs one update, returning its root.
pub fn show(
    app: &mut App,
    view: impl View<bevy_fynix::Bevy, EditorTheme>,
) -> Entity {
    let root = mount::<EditorTheme>(app.world_mut(), view);
    app.update();
    root
}

/// Every node under `node`, depth first, not counting `node`.
pub fn below(app: &App, node: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(node)
        .map(|kids| {
            kids.iter()
                .flat_map(|kid| {
                    let mut all = vec![kid];
                    all.extend(below(app, kid));
                    all
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The nodes under `node` that hold the component `C`.
pub fn all<C: Component>(app: &App, node: Entity) -> Vec<Entity> {
    below(app, node)
        .into_iter()
        .filter(|node| app.world().get::<C>(*node).is_some())
        .collect()
}

/// Opens the list of every dropdown and menu button under `node`.
/// A list is only built while it is open.
pub fn open_menus(app: &mut App, node: Entity) {
    for button in all::<MenuButton>(app, node) {
        app.world_mut().trigger(MenuEvent {
            source: button,
            action: MenuAction::Toggle,
        });
    }
    app.update();
}

/// The text of every editable field under `node`, in order.
pub fn inputs(app: &App, node: Entity) -> Vec<String> {
    all::<EditableText>(app, node)
        .into_iter()
        .map(|node| {
            app.world()
                .get::<EditableText>(node)
                .unwrap()
                .value()
                .to_string()
        })
        .collect()
}

/// The root of the first number or text field under `node`: the
/// parent of its first editable text.
pub fn field_root(app: &App, node: Entity) -> Entity {
    let input = all::<EditableText>(app, node)[0];
    app.world().get::<ChildOf>(input).unwrap().parent()
}

fn location() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 100,
            height: 100,
        },
        position: Vec2::ZERO,
    }
}

/// A primary-button drag of `target`, `dx` pixels from where it
/// began.
pub fn drag(app: &mut App, target: Entity, dx: f32) {
    let event = Drag {
        button: PointerButton::Primary,
        distance: Vec2::new(dx, 0.0),
        delta: Vec2::ZERO,
    };
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        event,
        target,
    ));
    app.update();
}

/// Fires the pointer event `event` on `on`.
fn fire<E>(app: &mut App, on: Entity, event: E)
where
    E: core::fmt::Debug + Clone + Reflect,
    Pointer<E>: bevy::ecs::event::Event,
    for<'t> <Pointer<E> as bevy::ecs::event::Event>::Trigger<'t>:
        Default,
{
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        event,
        on,
    ));
    app.update();
}

/// The start of a drag of `on` with `button`.
pub fn drag_start(app: &mut App, on: Entity, button: PointerButton) {
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    fire(app, on, DragStart { button, hit });
}

/// The end of a drag of `on`.
pub fn drag_stop(app: &mut App, on: Entity) {
    let event = DragEnd {
        button: PointerButton::Primary,
        distance: Vec2::ZERO,
    };
    fire(app, on, event);
}

/// A click of `on` with `button`, the `count`th in a row.
pub fn click(
    app: &mut App,
    on: Entity,
    button: PointerButton,
    count: u8,
) {
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    let event = Click {
        button,
        hit,
        duration: Duration::ZERO,
        count,
    };
    fire(app, on, event);
}

/// A primary-button press of `on`.
pub fn press(app: &mut App, on: Entity) {
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    let event = Press {
        button: PointerButton::Primary,
        hit,
        count: 1,
    };
    fire(app, on, event);
}

/// A primary-button drop of something on `on`.
pub fn drop_on(app: &mut App, on: Entity) {
    let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
    let event = DragDrop {
        button: PointerButton::Primary,
        dropped: Entity::PLACEHOLDER,
        hit,
    };
    fire(app, on, event);
}

/// A press of the key `code`, which types `key`.
pub fn key(app: &mut App, code: KeyCode, key: Key) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

/// A pair of fields, for a struct to nest.
#[derive(Reflect, Default, Clone, Debug, PartialEq)]
pub struct Inner {
    pub a: f32,
    pub b: f32,
}

/// An enum with a unit variant and two that carry fields.
#[derive(Reflect, Default, Clone, Debug, PartialEq)]
pub enum Kind {
    #[default]
    Dot,
    Circle {
        radius: f32,
    },
    Rect {
        width: f32,
        height: f32,
    },
}

/// A component with one of everything the inspector walks.
#[derive(Component, Reflect, Default, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct Probe {
    pub on: bool,
    pub level: f32,
    pub name: String,
    pub time: Duration,
    pub offset: Vec3,
    pub size: UVec2,
    pub inner: Inner,
    pub kind: Kind,
    pub items: Vec<f32>,
}

/// An app with [`Probe`] registered, and an entity carrying one.
pub fn probe_app() -> (App, Entity) {
    let mut app = app();
    app.register_type::<Probe>();
    let probe = app.world_mut().spawn(Probe::default()).id();
    (app, probe)
}

fn panel() -> impl View<Bevy, EditorTheme> {
    row((label("moxie"), button(label("go")).rules(ghost)))
}

#[test]
fn plugin_themes_a_mounted_view() {
    let mut app = App::new();
    app.add_plugins((TimePlugin, MoxieUiPlugin));
    mount::<EditorTheme>(app.world_mut(), panel());
    app.update();

    let mut labels = app.world_mut().query::<(&Text, &TextColor)>();
    let (_, color) = labels
        .iter(app.world())
        .find(|(text, _)| text.0 == "moxie")
        .unwrap();
    assert_eq!(color.0, EditorTheme::default().color.text);
}
