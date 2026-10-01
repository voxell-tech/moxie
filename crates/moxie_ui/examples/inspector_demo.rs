//! Demonstrates the reflection-driven inspector in
//! [`moxie_ui::inspector`], through the three views that mount one: a
//! resource, one component of an entity, and every component of an
//! entity at once.
//!
//! Along the way it shows what the field walk does with what it
//! finds: an editor per primitive, the compact per-axis row every
//! float/signed/unsigned glam vector gets instead of folding its
//! components away, the collapsible group a plain nested struct earns
//! for free by having no [`Inspect`](moxie_ui::inspector::Inspect) of
//! its own, a list with a row per item, and an enum whose fields
//! follow the variant.
//!
//! Edit any field; nothing here reacts to the values, so it's purely
//! a look at the editors themselves. Click any header to fold it.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, column, frame, label, row};
use bevy_fynix::{AnyView, Bevy, ViewExt as _, mount};
use moxie_ui::MoxieUiPlugin;
use moxie_ui::elements::{
    component_inspector_of, entity_inspector, resource_inspector_of,
};
use moxie_ui::inspector::InspectAppExt;
use moxie_ui::theme::EditorTheme;

fn main() {
    App::new()
        .add_plugins((
            // `../../assets`: the crates share the workspace's asset
            // folder rather than each carrying its own.
            DefaultPlugins.set(AssetPlugin {
                file_path: "../../assets".into(),
                ..default()
            }),
            MoxieUiPlugin,
        ))
        .register_type::<Showcase>()
        .register_type::<LocalTransform>()
        .register_inspectable::<Orbit>()
        .insert_resource(Showcase {
            samples: vec![0.25, 0.5, 1.0],
            ..default()
        })
        .add_systems(Startup, setup)
        .run();
}

/// Every editor the default registrations cover, plus a nested struct
/// with none of its own to show the fold, a list and an enum.
#[derive(Resource, Reflect, Default)]
#[reflect(Resource, Default)]
struct Showcase {
    visible: bool,
    brightness: f32,
    samples: Vec<f32>,
    tint: Vec4,
    uv_offset: Vec2,
    grid_size: IVec2,
    texture_size: UVec2,
    falloff: Falloff,
    local: LocalTransform,
}

/// No [`Inspect`](moxie_ui::inspector::Inspect) impl of its
/// own, so the inspector shows it as a collapsible group rather than
/// flattening its fields into `Showcase`'s own list.
#[derive(Reflect, Default)]
struct LocalTransform {
    translation: Vec3,
    scale: Vec3,
}

/// An enum: a dropdown of variants, with the active one's fields
/// under it.
#[derive(Reflect, Default)]
enum Falloff {
    #[default]
    None,
    Linear {
        distance: f32,
    },
    Smooth {
        start: f32,
        end: f32,
    },
}

/// A component of the demo's own, so the entity has something on it
/// besides what bevy puts there.
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
struct Orbit {
    radius: f32,
    speed: f32,
}

fn setup(world: &mut World) {
    world.spawn(Camera2d);

    // `Transform` also drags `GlobalTransform` in, but the entity
    // inspector only shows what's registered `register_inspectable` -
    // `GlobalTransform` is reflected, never opted in, so it stays
    // out.
    let subject = world
        .spawn((
            Transform::from_xyz(1.0, 2.0, 3.0),
            Orbit {
                radius: 4.0,
                speed: 0.5,
            },
        ))
        .id();

    mount::<EditorTheme>(
        world,
        row((
            panel(
                "Resource",
                resource_inspector_of::<Showcase>().boxed(),
            ),
            panel(
                "Component",
                component_inspector_of::<Transform>(subject).boxed(),
            ),
            panel("Entity", entity_inspector(subject).boxed()),
        ))
        .align(AlignItems::FlexStart)
        .gap(16.0)
        .padding(UiRect::all(px(24.0)))
        .width(percent(100.0))
        .height(percent(100.0)),
    );
}

/// A titled card, which is all these three have in common.
fn panel(
    title: &'static str,
    body: AnyView<Bevy, EditorTheme>,
) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let background = cx.theme().palette.base[1];
        cx.build(
            column((
                label(title).size(14.0).bold(true),
                frame().height(px(12.0)),
                body,
            ))
            .gap(0.0)
            .padding(UiRect::all(px(12.0)))
            .radius(8.0)
            .fill(background),
        )
    })
}
