//! Exports thirty frames of a moving cube with no window: PNG frames,
//! and an MP4 as well where `ffmpeg` is installed.
//!
//! `cargo run -p moxie_export --example export -- <output folder>`

use core::time::Duration;
use std::path::PathBuf;

use bevy::app::{RunMode, ScheduleRunnerPlugin};
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use bevy_motiongfx::BevyMotionGfxPlugin;
use bevy_motiongfx::prelude::*;
use moxie_export::{
    Codec, Export, ExportFinished, Output, Quality, find_ffmpeg,
    frame_count,
};

/// A width whose rows are padded on their way back from the GPU.
const SIZE: UVec2 = UVec2::new(500, 300);
/// A thirtieth of a second, to the nanosecond.
const STEP: Duration = Duration::from_nanos(33_333_333);
const FRAMES: u32 = 30;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                // Or the first frames are drawn before their
                // shaders are built, with nothing in them.
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin {
                run_mode: RunMode::Loop { wait: None },
            },
            BevyMotionGfxPlugin,
            moxie_export::plugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, exit_when_done)
        .add_observer(report)
        .run();
}

fn setup(
    mut commands: Commands,
    mut motiongfx: ResMut<MotionGfxManager>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let target = images.add(Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: Color::srgb(0.02, 0.02, 0.04).into(),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Transform::from_xyz(0.0, 2.0, 14.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // A 2D camera over the 3D one, as a project has.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
    ));
    commands.spawn((
        Sprite::from_color(Color::WHITE, Vec2::splat(40.0)),
        Transform::from_xyz(-200.0, 100.0, 0.0),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 10.0, 5.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let cube = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(Color::srgb(0.6, 0.9, 0.3))),
            Transform::from_xyz(-5.0, 0.0, 0.0),
        ))
        .id();

    let mut builder = motiongfx.create_builder();
    let track = builder
        .act(cube, path!(Transform.translation.x), |x| x + 10.0)
        .play(STEP * FRAMES)
        .compile();
    let timeline = builder.compile(track);
    let timeline = motiongfx.add_timeline(timeline);
    commands.spawn((timeline, RealtimePlayer::new()));

    let folder = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("export"), PathBuf::from);
    let frames = 0..frame_count(STEP * FRAMES, STEP);
    commands.spawn(Export::new(
        target.clone(),
        timeline,
        STEP,
        frames.clone(),
        Output::Images {
            dir: folder.join("frames"),
            stem: "cube".into(),
        },
    ));
    let Some(ffmpeg) = find_ffmpeg() else {
        info!("no ffmpeg found, so no video");
        return;
    };
    for codec in [Codec::H264, Codec::ProRes4444, Codec::Vp9] {
        commands.spawn(Export::new(
            target.clone(),
            timeline,
            STEP,
            frames.clone(),
            Output::Video {
                ffmpeg: ffmpeg.clone(),
                codec,
                quality: Quality::High,
                path: folder
                    .join("cube")
                    .with_extension(codec.extension()),
            },
        ));
    }
}

fn report(finished: On<ExportFinished>) {
    match &finished.result {
        Ok(()) => info!("export finished"),
        Err(error) => error!("export failed: {error}"),
    }
}

fn exit_when_done(
    exports: Query<(), With<Export>>,
    mut exit: MessageWriter<AppExit>,
) {
    if exports.is_empty() {
        exit.write(AppExit::Success);
    }
}
