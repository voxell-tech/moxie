//! The editor binary: [`MoxiePlugin`] over a blank project.
//!
//! `File > Open` is how a project gets its content, or a path given
//! as the first argument, which is opened at startup.

use bevy::prelude::*;
use bevy::window::WindowResolution;
use moxie::MoxiePlugin;

fn main() {
    App::new()
        .add_plugins((
            moxie::default_plugins().set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1920, 1080),
                    ..default()
                }),
                ..default()
            }),
            MoxiePlugin,
        ))
        .add_systems(Startup, open_argument)
        .run();
}

/// Opens the project named by the first command line argument, if
/// there is one.
fn open_argument(world: &mut World) {
    if let Some(path) = std::env::args_os().nth(1) {
        moxie::open_path(world, path.into());
    }
}
