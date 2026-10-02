//! The editor binary: [`MoxiePlugin`] over an empty scene.
//!
//! Nothing is spawned here beyond a camera and a light - `File >
//! Open` is how a project actually gets its content, or a path given
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
        .add_systems(Startup, (setup, open_argument).chain())
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera {
            order: 0,
            clear_color: Color::srgb(0.02, 0.02, 0.04).into(),
            ..default()
        },
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 14.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Draws over the 3D camera.
    commands.spawn((
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Camera2d,
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 10.0, 5.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Opens the project named by the first command line argument, if
/// there is one.
fn open_argument(world: &mut World) {
    if let Some(path) = std::env::args_os().nth(1) {
        moxie::open_path(world, path.into());
    }
}
