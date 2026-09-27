//! Offscreen previews of meshes and materials, for the asset picker.
//!
//! Each thumbnail gets a rig of its own - camera, subject and light -
//! on a render layer no other camera sees. The camera renders into the
//! thumbnail's image until the subject has loaded and been framed,
//! then the rig is despawned and the image kept.

use core::f32::consts::FRAC_PI_4;

use bevy::asset::LoadState;
use bevy::camera::RenderTarget;
use bevy::camera::primitives::MeshAabb as _;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use moxie_asset::AssetRef;
use moxie_ui::asset_picker::AssetPickerAppExt as _;

use crate::presets;

const SIZE: u32 = 128;
/// Clear of the scene's own layer and the editor UI's.
const FIRST_LAYER: usize = 8;
/// Frames a framed rig keeps rendering before it is torn down, so the
/// image has been written at least once.
const SETTLE_FRAMES: u8 = 3;
/// What a material is shown on.
const MATERIAL_SUBJECT: &str = "meshes/sphere.glb#Mesh0/Primitive0";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Studio>()
        .register_asset_thumbnail::<Mesh>(render_mesh)
        .register_asset_thumbnail::<StandardMaterial>(render_material)
        .add_systems(Update, develop);
}

/// What every rig shares.
#[derive(Resource)]
struct Studio {
    /// The render layer the next rig gets.
    next_layer: usize,
    /// One light for every rig, on each live rig's layer: directional
    /// lights are capped per world whatever layer they are on.
    light: Option<Entity>,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            next_layer: FIRST_LAYER,
            light: None,
        }
    }
}

/// A thumbnail rig's camera.
#[derive(Component)]
pub(crate) struct ThumbnailCamera {
    subject: Entity,
    layer: usize,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// Frames rendered since the subject was framed, `None` before.
    settled: Option<u8>,
}

fn render_mesh(
    world: &mut World,
    asset: &AssetRef,
) -> Option<Handle<Image>> {
    let mesh = asset.handle(world.resource::<AssetServer>());
    Some(rig(world, mesh, presets::DEFAULT_MATERIAL))
}

fn render_material(
    world: &mut World,
    asset: &AssetRef,
) -> Option<Handle<Image>> {
    let assets = world.resource::<AssetServer>();
    let mesh = assets.load(MATERIAL_SUBJECT);
    let material = asset.handle(assets);
    Some(rig(world, mesh, material))
}

/// Spawns a rig showing `mesh` in `material`, and returns the image it
/// renders into.
fn rig(
    world: &mut World,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
) -> Handle<Image> {
    let image = world.resource_mut::<Assets<Image>>().add(
        Image::new_target_texture(
            SIZE,
            SIZE,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ),
    );

    let mut studio = world.resource_mut::<Studio>();
    let index = studio.next_layer;
    studio.next_layer += 1;
    let layer = RenderLayers::layer(index);

    let light = match studio.light {
        Some(light) => light,
        None => {
            let light = world
                .spawn((
                    DirectionalLight::default(),
                    Transform::from_xyz(2.0, 4.0, 3.0)
                        .looking_at(Vec3::ZERO, Vec3::Y),
                    RenderLayers::none(),
                ))
                .id();
            world.resource_mut::<Studio>().light = Some(light);
            light
        }
    };
    if let Some(mut layers) = world.get_mut::<RenderLayers>(light) {
        *layers = layers.clone().with(index);
    }

    let subject = world
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            layer.clone(),
        ))
        .id();
    world.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Transform::from_xyz(0.0, 0.0, 4.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        layer,
        ThumbnailCamera {
            subject,
            layer: index,
            mesh,
            material,
            settled: None,
        },
    ));

    image
}

/// Frames each rig's subject once it has loaded, and tears the rig
/// down once it has rendered.
fn develop(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    studio: Res<Studio>,
    mut lights: Query<&mut RenderLayers, Without<ThumbnailCamera>>,
    mut cameras: Query<(
        Entity,
        &mut ThumbnailCamera,
        &mut Transform,
        &Projection,
    )>,
) {
    for (camera, mut rig, mut transform, projection) in &mut cameras {
        let light =
            studio.light.and_then(|light| lights.get_mut(light).ok());
        match rig.settled {
            None => {
                let failed = matches!(
                    assets.load_state(&rig.mesh),
                    LoadState::Failed(_)
                ) || matches!(
                    assets.load_state(&rig.material),
                    LoadState::Failed(_)
                );
                if failed {
                    tear_down(&mut commands, light, camera, &rig);
                    continue;
                }
                // Through `Assets` rather than the server: a material
                // kept under a UUID was never loaded, so never finishes.
                if !materials.contains(&rig.material) {
                    continue;
                }
                let Some(aabb) = meshes
                    .get(&rig.mesh)
                    .and_then(|mesh| mesh.compute_aabb())
                else {
                    continue;
                };

                let fov = match projection {
                    Projection::Perspective(perspective) => {
                        perspective.fov
                    }
                    _ => FRAC_PI_4,
                };
                let center = Vec3::from(aabb.center);
                let radius =
                    Vec3::from(aabb.half_extents).length().max(0.01);
                let distance = radius / (fov / 2.0).sin();
                let direction = Vec3::new(1.0, 0.8, 1.4).normalize();
                *transform = Transform::from_translation(
                    center + direction * distance,
                )
                .looking_at(center, Vec3::Y);
                rig.settled = Some(0);
            }
            Some(frames) if frames >= SETTLE_FRAMES => {
                tear_down(&mut commands, light, camera, &rig);
            }
            Some(frames) => rig.settled = Some(frames + 1),
        }
    }
}

fn tear_down(
    commands: &mut Commands,
    light: Option<Mut<RenderLayers>>,
    camera: Entity,
    rig: &ThumbnailCamera,
) {
    if let Some(mut layers) = light {
        *layers = layers.clone().without(rig.layer);
    }
    commands.entity(rig.subject).despawn();
    commands.entity(camera).despawn();
}
