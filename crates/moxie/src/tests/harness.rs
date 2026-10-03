//! The whole editor, headless, for tests: no window, no GPU, and
//! frames stepped by hand.
//!
//! With no window nothing is laid out, so nothing is found by where
//! it is drawn. A test finds a control by the text it shows, or a
//! tooltip's name, and acts on it the way a click or a key would.

use core::time::Duration;

use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::log::LogPlugin;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Click, Drag, DragEnd, Pointer};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::sync_world::SyncWorldPlugin;
use bevy::ui_widgets::{
    Activate, Button as ButtonBehavior, MenuAction, MenuEvent,
    MenuItem,
};
use bevy::window::{ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use bevy_fynix::Leaving;

use crate::MoxiePlugin;

/// Frames an interaction is given to settle: its commands to apply,
/// and the UI it changes to rebuild.
pub(crate) const SETTLE: usize = 3;

pub(crate) struct Editor {
    pub(crate) app: App,
}

impl Editor {
    pub(crate) fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            crate::default_plugins()
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
                // Tests run side by side, and only one of them could
                // own the global logger.
                .disable::<LogPlugin>(),
            // Rendering with no GPU never adds this, but the hooks
            // on anything drawable still reach for what
            // it keeps.
            SyncWorldPlugin,
            MoxiePlugin,
        ));
        app.finish();
        app.cleanup();
        // Keys reach whatever has focus by way of the primary window,
        // so there has to be one, drawn or not.
        app.world_mut().spawn((Window::default(), PrimaryWindow));

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

    /// Every visible entity showing exactly `text`. A menu fading
    /// out after it shut still has its rows, which don't count.
    pub(crate) fn texts(&mut self, text: &str) -> Vec<Entity> {
        let world = self.world();
        let shown = world
            .query::<(Entity, &Text, &InheritedVisibility)>()
            .iter(world)
            .filter(|(_, shown, visible)| {
                shown.0 == text && visible.get()
            })
            .map(|(entity, ..)| entity)
            .collect::<Vec<_>>();
        shown
            .into_iter()
            .filter(|&entity| {
                let mut at = entity;
                loop {
                    if world.get::<Leaving>(at).is_some() {
                        return false;
                    }
                    match world.get::<ChildOf>(at) {
                        Some(parent) => at = parent.parent(),
                        None => return true,
                    }
                }
            })
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
        // A click on a menu row also sends this after activating it.
        if world.get::<MenuItem>(at).is_some() {
            world.trigger(MenuEvent {
                source: at,
                action: MenuAction::CloseAll,
            });
        }
        self.step(SETTLE);
    }

    /// Clicks the entity showing `text`, `count` times in a row. The
    /// click bubbles up from there, as a real one does.
    pub(crate) fn click(&mut self, text: &str, count: u8) {
        let entity = self.text(text);
        self.click_entity(entity, count);
    }

    /// Clicks `entity` `count` times in a row.
    pub(crate) fn click_entity(&mut self, entity: Entity, count: u8) {
        self.pointer(
            entity,
            Click {
                button: PointerButton::Primary,
                hit: HitData::new(
                    Entity::PLACEHOLDER,
                    0.0,
                    None,
                    None,
                ),
                duration: Duration::ZERO,
                count,
            },
        );
        self.step(SETTLE);
    }

    /// Drags `entity` `dx` pixels to the right and lets go.
    pub(crate) fn drag(&mut self, entity: Entity, dx: f32) {
        let button = PointerButton::Primary;
        let distance = Vec2::new(dx, 0.0);
        self.pointer(
            entity,
            Drag {
                button,
                distance,
                delta: distance,
            },
        );
        self.step(1);
        self.pointer(entity, DragEnd { button, distance });
        self.step(SETTLE);
    }

    /// Sends `event` from the mouse to `entity`, bubbling up from
    /// there as a real one does.
    fn pointer<E: Clone + core::fmt::Debug + Reflect>(
        &mut self,
        entity: Entity,
        event: E,
    ) {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        self.world().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            event,
            entity,
        ));
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
