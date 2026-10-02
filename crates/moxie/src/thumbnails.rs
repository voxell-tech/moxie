//! Offscreen previews of meshes and materials, for the asset picker.
//!
//! Each thumbnail gets a rig of its own - camera, subject and light -
//! on a render layer no other camera sees. Asking for one only queues
//! it: rigs start a few a frame, so a picker listing hundreds of
//! assets doesn't stall. A rig's camera stays off until its subject
//! has loaded and been framed, renders into the thumbnail's image for
//! a few frames, and then the rig is despawned and the image kept.

use core::f32::consts::FRAC_PI_4;
use std::collections::VecDeque;

use bevy::asset::LoadState;
use bevy::camera::RenderTarget;
use bevy::camera::primitives::MeshAabb as _;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use moxie_asset::{AssetRef, AssetTypeAppExt as _};

use crate::presets;

const SIZE: u32 = 128;
/// Clear of the scene's own layer and the editor UI's.
const FIRST_LAYER: usize = 8;
/// Rigs alive at once, waiting or rendering.
const MAX_LIVE: usize = 8;
/// Rigs started in one frame.
const START_PER_FRAME: usize = 2;
/// Frames a rig may wait for its subject before it is given up on: a
/// material removed while it waits never loads, and never fails
/// either.
const MAX_WAIT: u32 = 600;
/// Frames a framed rig keeps rendering before it is torn down, so the
/// image has been written at least once.
const SETTLE_FRAMES: u8 = 3;
/// What a material is shown on.
const MATERIAL_SUBJECT: &str = "meshes/sphere.glb#Mesh0/Primitive0";

pub(crate) fn plugin(app: &mut App) {
    app.asset_type::<Mesh>().thumbnail = Some(render_mesh);
    app.asset_type::<StandardMaterial>().thumbnail =
        Some(render_material);
    app.init_resource::<Studio>()
        .add_systems(Update, (start_rigs, develop).chain());
}

/// What every rig shares.
#[derive(Resource)]
struct Studio {
    /// Thumbnails asked for and not yet started.
    queue: VecDeque<Shot>,
    /// Rigs alive now.
    live: usize,
    /// Render layers a torn-down rig gave back.
    free_layers: Vec<usize>,
    /// The render layer the next rig gets when none is free.
    next_layer: usize,
    /// One light for every rig, on each live rig's layer:
    /// directional lights are capped per world whatever layer
    /// they are on.
    light: Option<Entity>,
}

impl Default for Studio {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            live: 0,
            free_layers: Vec::new(),
            next_layer: FIRST_LAYER,
            light: None,
        }
    }
}

/// A thumbnail waiting for a rig.
struct Shot {
    image: Handle<Image>,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// A thumbnail rig's camera.
#[derive(Component)]
pub(crate) struct ThumbnailCamera {
    subject: Entity,
    layer: usize,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// Frames spent waiting for the subject to load.
    waited: u32,
    /// Frames rendered since the subject was framed, `None` before.
    settled: Option<u8>,
}

fn render_mesh(
    world: &mut World,
    asset: &AssetRef,
) -> Option<Handle<Image>> {
    let mesh = asset.handle(world.resource::<AssetServer>());
    Some(queue(world, mesh, presets::DEFAULT_MATERIAL))
}

fn render_material(
    world: &mut World,
    asset: &AssetRef,
) -> Option<Handle<Image>> {
    let assets = world.resource::<AssetServer>();
    let mesh = assets.load(MATERIAL_SUBJECT);
    let material = asset.handle(assets);
    Some(queue(world, mesh, material))
}

/// Queues a thumbnail of `mesh` in `material`, and returns the image
/// it will render into. The image stays blank until its rig has run.
fn queue(
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
    world.resource_mut::<Studio>().queue.push_back(Shot {
        image: image.clone(),
        mesh,
        material,
    });
    image
}

/// Starts queued rigs, a few a frame and only while there's room.
fn start_rigs(world: &mut World) {
    for _ in 0..START_PER_FRAME {
        let mut studio = world.resource_mut::<Studio>();
        if studio.live >= MAX_LIVE {
            return;
        }
        let Some(shot) = studio.queue.pop_front() else {
            return;
        };
        studio.live += 1;
        let index = studio.free_layers.pop().unwrap_or_else(|| {
            studio.next_layer += 1;
            studio.next_layer - 1
        });
        rig(world, shot, index);
    }
}

/// Spawns a rig for `shot` on render layer `index`.
fn rig(world: &mut World, shot: Shot, index: usize) {
    let Shot {
        image,
        mesh,
        material,
    } = shot;
    let layer = RenderLayers::layer(index);

    let light = match world.resource::<Studio>().light {
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
            // Nothing worth drawing until the subject is framed.
            is_active: false,
            ..default()
        },
        RenderTarget::Image(image.into()),
        Transform::from_xyz(0.0, 0.0, 4.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        layer,
        ThumbnailCamera {
            subject,
            layer: index,
            mesh,
            material,
            waited: 0,
            settled: None,
        },
    ));
}

/// Frames each rig's subject once it has loaded, and tears the rig
/// down once it has rendered, or once it's clear it never will.
fn develop(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    mut studio: ResMut<Studio>,
    mut lights: Query<&mut RenderLayers, Without<ThumbnailCamera>>,
    mut cameras: Query<(
        Entity,
        &mut ThumbnailCamera,
        &mut Camera,
        &mut Transform,
        &Projection,
    )>,
) {
    for (entity, mut rig, mut camera, mut transform, projection) in
        &mut cameras
    {
        let done = match rig.settled {
            None => {
                rig.waited += 1;
                let failed = matches!(
                    assets.load_state(&rig.mesh),
                    LoadState::Failed(_)
                ) || matches!(
                    assets.load_state(&rig.material),
                    LoadState::Failed(_)
                );
                // Through `Assets` rather than the server: a material
                // kept under a UUID was never loaded, so never
                // finishes.
                let loaded = meshes
                    .get(&rig.mesh)
                    .filter(|_| materials.contains(&rig.material));
                match loaded.map(|mesh| mesh.compute_aabb()) {
                    // A mesh with no positions has nothing to frame.
                    Some(None) => true,
                    Some(Some(aabb)) => {
                        let fov = match projection {
                            Projection::Perspective(perspective) => {
                                perspective.fov
                            }
                            _ => FRAC_PI_4,
                        };
                        let center = Vec3::from(aabb.center);
                        let radius = Vec3::from(aabb.half_extents)
                            .length()
                            .max(0.01);
                        let distance = radius / (fov / 2.0).sin();
                        let direction =
                            Vec3::new(1.0, 0.8, 1.4).normalize();
                        *transform = Transform::from_translation(
                            center + direction * distance,
                        )
                        .looking_at(center, Vec3::Y);
                        camera.is_active = true;
                        rig.settled = Some(0);
                        false
                    }
                    None => failed || rig.waited > MAX_WAIT,
                }
            }
            Some(frames) if frames >= SETTLE_FRAMES => true,
            Some(frames) => {
                rig.settled = Some(frames + 1);
                false
            }
        };
        if !done {
            continue;
        }

        if let Some(mut layers) =
            studio.light.and_then(|light| lights.get_mut(light).ok())
        {
            *layers = layers.clone().without(rig.layer);
        }
        studio.free_layers.push(rig.layer);
        studio.live -= 1;
        commands.entity(rig.subject).despawn();
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use bevy::asset::uuid::Uuid;

    use super::*;
    use crate::tests::harness::Editor;

    fn rigs(editor: &mut Editor) -> usize {
        let world = editor.world();
        world.query::<&ThumbnailCamera>().iter(world).count()
    }

    fn studio(editor: &mut Editor) -> &Studio {
        editor.world().resource::<Studio>()
    }

    #[test]
    fn rigs_start_a_few_at_a_time_and_share_layers() {
        let mut editor = Editor::new();
        for (_, path) in presets::MESHES.iter().cycle().take(40) {
            render_mesh(
                editor.world(),
                &AssetRef::Path(path.to_string()),
            );
        }

        editor.step(1);
        assert_eq!(rigs(&mut editor), START_PER_FRAME);

        // Meshes load off the main thread, so give them real time.
        for _ in 0..2_000 {
            editor.step(1);
            assert!(rigs(&mut editor) <= MAX_LIVE);
            let studio = studio(&mut editor);
            if studio.queue.is_empty() && studio.live == 0 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }

        let studio = studio(&mut editor);
        assert!(
            studio.queue.is_empty() && studio.live == 0,
            "all ran"
        );
        assert!(
            studio.next_layer - FIRST_LAYER <= MAX_LIVE,
            "layers are reused",
        );
    }

    #[test]
    fn a_rig_that_never_loads_is_given_up_on() {
        let mut editor = Editor::new();
        // A material that is nowhere and never will be.
        let missing =
            Handle::<StandardMaterial>::from(Uuid::new_v4());
        let mesh = editor
            .world()
            .resource::<AssetServer>()
            .load(MATERIAL_SUBJECT);
        queue(editor.world(), mesh, missing);

        editor.step(2);
        assert_eq!(rigs(&mut editor), 1);
        editor.step(MAX_WAIT as usize + 2);
        assert_eq!(rigs(&mut editor), 0);
        assert_eq!(studio(&mut editor).live, 0);
    }
}
