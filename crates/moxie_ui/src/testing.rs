//! What the tests share: a headless app with the plugin in it, the
//! reflected types the inspector tests edit, and the lookups they
//! reach for.

use core::time::Duration;

use bevy::app::{App, TaskPoolPlugin};
use bevy::asset::AssetPlugin;
use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::hierarchy::Children;
use bevy::input::InputPlugin;
use bevy::input_focus::{InputDispatchPlugin, InputFocusPlugin};
use bevy::picking::backend::HitData;
use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use bevy::ui::UiScale;
use bevy::window::PrimaryWindow;
use bevy_fynix::{View, mount};

use crate::MoxieUiPlugin;
use crate::theme::EditorTheme;

/// A headless app under the editor theme, whose clock moves 50ms per
/// update.
pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
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

/// What every editable field under `node` shows, in order.
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
