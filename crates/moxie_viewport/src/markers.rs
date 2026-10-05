//! Scene cameras and directional lights, drawn in a viewport as
//! lines and picked by them.

use core::f32::consts::TAU;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_fynix::Theme;
use moxie_ui::SelectedEntity;
use moxie_ui::theme::EditorTheme;

use super::camera::Lens;
use super::{EditorGizmos, OutputAspect, gizmo};

/// The rays drawn off the rim of a directional light's disc.
const LIGHT_RAYS: usize = 8;
/// The sides of the polygon a directional light's disc is drawn as.
const LIGHT_RIM: usize = 32;

/// Draws what each 3D scene camera sees as a frame it looks out
/// through, and where each directional light shines as rays off a
/// disc. The selected one is in the accent colour.
pub(crate) fn draw_cameras_and_lights(
    mut gizmos: Gizmos<EditorGizmos>,
    selected: Res<SelectedEntity>,
    markers: Markers,
) {
    let accent = markers.theme.0.color.accent;
    for (entity, color, lines) in markers.all() {
        let color = if selected.0 == Some(entity) {
            accent
        } else {
            color
        };
        for [from, to] in lines {
            gizmos.line(from, to, color);
        }
    }
}

/// The subjects a viewport draws as lines for want of a mesh: the 3D
/// scene cameras and the directional lights.
#[derive(SystemParam)]
pub(crate) struct Markers<'w, 's> {
    theme: Res<'w, Theme<EditorTheme>>,
    aspect: Res<'w, OutputAspect>,
    cameras: Query<
        'w,
        's,
        (Entity, &'static GlobalTransform, &'static Projection),
        Lens,
    >,
    lights: Query<
        'w,
        's,
        (Entity, &'static GlobalTransform),
        With<DirectionalLight>,
    >,
}

impl Markers<'_, '_> {
    /// Each one's subject, colour and lines, in the world.
    fn all(&self) -> Vec<(Entity, Color, Vec<[Vec3; 2]>)> {
        let style = &self.theme.0.gizmo;
        let aspect = self.aspect.0;
        let cameras =
            self.cameras.iter().map(|(entity, pose, projection)| {
                let lines = camera_lines(
                    &pose.compute_transform(),
                    projection,
                    aspect,
                    style.camera_depth,
                );
                (entity, style.camera, lines)
            });
        let lights = self.lights.iter().map(|(entity, pose)| {
            let lines = light_lines(
                &pose.compute_transform(),
                style.light_radius,
                style.light_ray,
            );
            (entity, style.light, lines)
        });
        cameras.chain(lights).collect()
    }

    /// The one whose lines the `cursor` is nearest, within reach of
    /// a click. `project` takes a point in the world to where the
    /// cursor's viewport shows it.
    pub(crate) fn under(
        &self,
        cursor: Vec2,
        project: impl Fn(Vec3) -> Option<Vec2>,
    ) -> Option<Entity> {
        let reach = self.theme.0.gizmo.pick_radius;
        self.all()
            .into_iter()
            .filter_map(|(entity, _, lines)| {
                let nearest = lines
                    .into_iter()
                    .filter_map(|[from, to]| {
                        Some(gizmo::segment_distance(
                            cursor,
                            project(from)?,
                            project(to)?,
                        ))
                    })
                    .min_by(f32::total_cmp)?;
                Some((entity, nearest))
            })
            .filter(|(_, distance)| *distance <= reach)
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(entity, _)| entity)
    }
}

/// The frame a camera at `pose` looks out through, `depth` ahead of
/// it and `aspect` times as wide as tall, under a mark of which way
/// is up.
fn camera_lines(
    pose: &Transform,
    projection: &Projection,
    aspect: f32,
    depth: f32,
) -> Vec<[Vec3; 2]> {
    // The half size of what it sees where it stands, and that far
    // ahead.
    let (near, far) = match projection {
        Projection::Perspective(lens) => {
            let height = depth * (lens.fov / 2.0).tan();
            (Vec2::ZERO, Vec2::new(height * aspect, height))
        }
        Projection::Orthographic(lens) => {
            (lens.area.half_size(), lens.area.half_size())
        }
        Projection::Custom(_) => return Vec::new(),
    };
    let at = |point: Vec3| pose.translation + pose.rotation * point;
    let corners = |half: Vec2, z: f32| {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| at(Vec3::new(half.x * x, half.y * y, z)))
    };
    let front = corners(near, 0.0);
    let back = corners(far, -depth);
    let up = [(-0.4, 1.1), (0.0, 1.5), (0.4, 1.1)]
        .map(|(x, y)| at(Vec3::new(far.x * x, far.y * y, -depth)));
    let mut lines = Vec::new();
    for corner in 0..4 {
        let next = (corner + 1) % 4;
        lines.push([front[corner], back[corner]]);
        lines.push([back[corner], back[next]]);
        lines.push([front[corner], front[next]]);
    }
    for point in 0..3 {
        lines.push([up[point], up[(point + 1) % 3]]);
    }
    lines
}

/// A disc of `radius` at `pose`, and rays of `length` off it the way
/// a light there shines.
fn light_lines(
    pose: &Transform,
    radius: f32,
    length: f32,
) -> Vec<[Vec3; 2]> {
    let rim = |at: usize| {
        let turn = at as f32 / LIGHT_RIM as f32 * TAU;
        pose.translation
            + pose.rotation
                * Vec3::new(turn.cos(), turn.sin(), 0.0)
                * radius
    };
    let ray = pose.rotation * Vec3::NEG_Z * length;
    let mut lines = vec![[pose.translation, pose.translation + ray]];
    for at in 0..LIGHT_RIM {
        lines.push([rim(at), rim(at + 1)]);
        if at % (LIGHT_RIM / LIGHT_RAYS) == 0 {
            lines.push([rim(at), rim(at) + ray]);
        }
    }
    lines
}
