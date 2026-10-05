//! The transform gizmo: handles drawn over the selection in a
//! viewport, dragged to move, turn or scale it.

mod drag;
mod fills;
mod paint;

use core::f32::consts::PI;

use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::Theme;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, ShortcutAppExt as _,
};
use moxie_ui::SelectedEntity;
use moxie_ui::inspector::Field;
use moxie_ui::theme::{EditorTheme, GizmoStyle};

pub(super) use self::drag::watch;
use self::drag::{ActiveDrag, Pending, drive};
use self::fills::{FillMaterial, Fills, HandleFills, spawn_fills};
use self::paint::{Shown, paint};
use super::camera::VIEWPORT;
use super::{EDITOR_LAYER, EditorCamera};

/// Segments a rotation ring is drawn and picked by.
const RING_STEPS: usize = 48;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<HandleGizmos>()
        .add_plugins((
            MaterialPlugin::<FillMaterial>::default(),
            add_mode_commands,
        ))
        .init_resource::<GizmoSettings>()
        .init_resource::<ActiveDrag>()
        .add_systems(Startup, (style_handles, spawn_fills))
        .add_systems(Update, drive.after(super::place_cameras))
        .add_systems(
            PostUpdate,
            handles
                .in_set(super::Overlay)
                .before(VisibilitySystems::VisibilityPropagate),
        );
}

/// The gizmo's handles, drawn over the scene.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct HandleGizmos;

fn style_handles(
    mut store: ResMut<GizmoConfigStore>,
    theme: Res<Theme<EditorTheme>>,
) {
    let (config, _) = store.config_mut::<HandleGizmos>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
    config.line.width = theme.0.gizmo.line_width;
    // In front of whatever the handles sit inside.
    config.depth_bias = -1.0;
}

/// The part of a [`Transform`] the gizmo edits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

impl GizmoMode {
    pub(crate) const ALL: [Self; 3] =
        [Self::Translate, Self::Rotate, Self::Scale];

    fn field(self, entity: Entity) -> Field {
        Field::of::<Transform>(entity).child(match self {
            Self::Translate => "translation",
            Self::Rotate => "rotation",
            Self::Scale => "scale",
        })
    }
}

/// The axes the gizmo's handles follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GizmoSpace {
    #[default]
    World,
    Local,
}

impl GizmoSpace {
    pub(crate) const ALL: [Self; 2] = [Self::World, Self::Local];
}

/// The gizmo every viewport shows.
#[derive(Resource, Clone, Copy, Default, PartialEq)]
pub(crate) struct GizmoSettings {
    pub(crate) mode: GizmoMode,
    pub(crate) space: GizmoSpace,
}

/// The commands that pick the gizmo's mode, in the order of
/// [`GizmoMode::ALL`].
pub(crate) const MODE_COMMANDS: [CommandId; 3] = [
    CommandId("gizmo.mode.translate"),
    CommandId("gizmo.mode.rotate"),
    CommandId("gizmo.mode.scale"),
];

fn add_mode_commands(app: &mut App) {
    let pick = |mode: usize, label, key, run| {
        (
            CommandSpec {
                id: MODE_COMMANDS[mode],
                label,
                scope: VIEWPORT,
                run,
                // A drag keeps the mode it began in, or it would
                // write one field under the handles of another.
                enabled: |world| {
                    world.resource::<ActiveDrag>().0.is_none()
                },
                repeat: false,
            },
            Chord::key(key),
        )
    };
    fn set(world: &mut World, mode: GizmoMode) {
        world.resource_mut::<GizmoSettings>().mode = mode;
    }
    for (command, chord) in [
        pick(0, "Move", KeyCode::KeyW, |world, _| {
            set(world, GizmoMode::Translate);
        }),
        pick(1, "Rotate", KeyCode::KeyE, |world, _| {
            set(world, GizmoMode::Rotate);
        }),
        pick(2, "Scale", KeyCode::KeyR, |world, _| {
            set(world, GizmoMode::Scale);
        }),
    ] {
        app.add_command(command, &[chord]);
    }
}

/// One handle of the gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Along or around the axis of this index.
    Axis(usize),
    /// Along the two axes the axis of this index is square to.
    Plane(usize),
    /// At the origin: across the view for a translation, every axis
    /// at once for a scale.
    Centre,
    /// Around the line of sight.
    View,
    /// Any way round, as a ball under the pointer rolls.
    Ball,
}

/// The handle of a viewport's gizmo under its pointer. On the
/// viewport's camera.
#[derive(Component, Default)]
pub(crate) struct Hot(pub(crate) Option<Handle>);

/// An arc of a unit circle, from the angle `from` to the angle `to`.
#[derive(Clone, Copy, Debug)]
struct Arc {
    /// The way to the point at no angle.
    start: Vec3,
    /// The way to the point a quarter turn on.
    quarter: Vec3,
    from: f32,
    to: f32,
}

impl Arc {
    /// The whole circle around `normal`.
    fn full(normal: Vec3) -> Self {
        let start = normal.any_orthonormal_vector();
        Self {
            start,
            quarter: normal.cross(start),
            from: -PI,
            to: PI,
        }
    }

    /// The half of the circle around `normal` nearer to a viewer
    /// looking along `look`, and as much of the rest as is within
    /// `overlap` behind the centre.
    fn facing(normal: Vec3, look: Vec3, overlap: f32) -> Self {
        let away = look - normal * normal.dot(look);
        let lean = away.length();
        if lean <= overlap.max(f32::EPSILON) {
            return Self::full(normal);
        }
        let start = -away / lean;
        let half = (-overlap / lean).acos();
        Self {
            start,
            quarter: normal.cross(start),
            from: -half,
            to: half,
        }
    }

    fn at(&self, angle: f32) -> Vec3 {
        self.start * angle.cos() + self.quarter * angle.sin()
    }

    /// The ends of `steps` segments along it, `radius` from `centre`.
    fn points(
        self,
        centre: Vec3,
        radius: f32,
        steps: usize,
    ) -> impl Iterator<Item = Vec3> {
        (0..=steps).map(move |step| {
            let along = step as f32 / steps as f32;
            let angle = self.from + (self.to - self.from) * along;
            centre + self.at(angle) * radius
        })
    }
}

/// How much of a handle shows: none of it with `lean` at the first
/// of `range` or under, all of it from the second up.
fn fade(lean: f32, [gone, full]: [f32; 2]) -> f32 {
    ((lean - gone) / (full - gone)).clamp(0.0, 1.0)
}

/// Where a gizmo sits and how large it is, in the world.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: Vec3,
    axes: [Vec3; 3],
    /// The length of a logical pixel at the origin.
    scale: f32,
    /// The way the viewer looks at the origin.
    look: Vec3,
    /// The rotation of the camera.
    facing: Quat,
}

impl Frame {
    /// The gizmo on `target` as `camera` draws it.
    fn of(
        target: &GlobalTransform,
        settings: GizmoSettings,
        camera: &Camera,
        view: &GlobalTransform,
    ) -> Option<Self> {
        let origin = target.translation();
        let at = camera.world_to_viewport(view, origin).ok()?;
        let look =
            *camera.viewport_to_world(view, at).ok()?.direction;
        let beside = camera
            .world_to_viewport(
                view,
                origin + view.rotation() * Vec3::X,
            )
            .ok()?;
        let per_unit = at.distance(beside);
        if per_unit <= f32::EPSILON {
            return None;
        }
        // A scale is along the subject's own axes, whatever the
        // space.
        let local = settings.space == GizmoSpace::Local
            || settings.mode == GizmoMode::Scale;
        let rotation = if local {
            target.rotation()
        } else {
            Quat::IDENTITY
        };
        Some(Self {
            origin,
            axes: Vec3::AXES.map(|axis| rotation * axis),
            scale: 1.0 / per_unit,
            look,
            facing: view.rotation(),
        })
    }

    /// The point `pixels` along `axis` from the origin.
    fn along(&self, axis: usize, pixels: f32) -> Vec3 {
        self.origin + self.axes[axis] * pixels * self.scale
    }

    /// The two axes the plane handle of `axis` lies along.
    fn sides(&self, axis: usize) -> [Vec3; 2] {
        [self.axes[(axis + 1) % 3], self.axes[(axis + 2) % 3]]
    }

    /// The corners of the plane handle of `axis`, in turn.
    fn plane(&self, axis: usize, style: &GizmoStyle) -> [Vec3; 4] {
        let [a, b] = self.sides(axis);
        let middle =
            self.origin + (a + b) * style.plane_offset * self.scale;
        let half = style.plane_size / 2.0 * self.scale;
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| middle + (a * x + b * y) * half)
    }

    /// The points of the circle `arc` is of, `pixels` from the
    /// origin.
    fn ring(
        &self,
        arc: Arc,
        pixels: f32,
    ) -> impl Iterator<Item = Vec3> {
        arc.points(self.origin, pixels * self.scale, RING_STEPS)
    }

    /// The part of the rotation ring of `axis` that is drawn.
    fn arc(&self, axis: usize, style: &GizmoStyle) -> Arc {
        Arc::facing(self.axes[axis], self.look, style.ring_overlap)
    }

    /// How much of the handle along `axis` shows, which is less as
    /// it comes to point at the viewer.
    fn axis_fade(&self, axis: usize, style: &GizmoStyle) -> f32 {
        let lean = 1.0 - self.axes[axis].dot(self.look).abs();
        fade(lean, style.axis_fade)
    }

    /// How much of the plane handle of `axis` shows, which is less
    /// as it comes to be seen edge on.
    fn plane_fade(&self, axis: usize, style: &GizmoStyle) -> f32 {
        fade(self.axes[axis].dot(self.look).abs(), style.plane_fade)
    }
}

/// Whether `point` is inside the convex outline through `corners`.
fn inside(point: Vec2, corners: &[Vec2]) -> bool {
    let turns = (0..corners.len()).map(|at| {
        let from = corners[at];
        let to = corners[(at + 1) % corners.len()];
        (to - from).perp_dot(point - from)
    });
    turns.clone().all(|turn| turn >= 0.0)
        || turns.clone().all(|turn| turn <= 0.0)
}

/// The distance from `point` to the segment from `a` to `b`.
pub(super) fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let length = along.length_squared();
    if length <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(along) / length).clamp(0.0, 1.0);
    point.distance(a + along * t)
}

/// The handle of the axis nearest by `distance`, of those within
/// `reach`.
fn nearest_axis(
    reach: f32,
    distance: impl Fn(usize) -> Option<f32>,
) -> Option<Handle> {
    (0..3)
        .filter_map(|axis| Some((axis, distance(axis)?)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(axis, _)| Handle::Axis(axis))
}

/// The handle of `frame` under `cursor`, `project` taking a point in
/// the world to where the cursor's viewport shows it.
fn pick(
    mode: GizmoMode,
    frame: &Frame,
    cursor: Vec2,
    style: &GizmoStyle,
    project: impl Fn(Vec3) -> Option<Vec2>,
) -> Option<Handle> {
    let from_centre = cursor.distance(project(frame.origin)?);
    if mode == GizmoMode::Rotate {
        let ring = |arc: Arc, pixels: f32| {
            let ring = frame
                .ring(arc, pixels)
                .map(&project)
                .collect::<Option<Vec<_>>>()?;
            ring.windows(2)
                .map(|ends| {
                    segment_distance(cursor, ends[0], ends[1])
                })
                .min_by(f32::total_cmp)
        };
        return nearest_axis(style.pick_radius, |axis| {
            ring(frame.arc(axis, style), style.size)
        })
        .or_else(|| {
            ring(Arc::full(frame.look), style.outer_ring)
                .filter(|distance| *distance <= style.pick_radius)
                .map(|_| Handle::View)
        })
        .or_else(|| {
            (from_centre <= style.size).then_some(Handle::Ball)
        });
    }

    if from_centre <= style.centre.max(style.pick_radius) {
        return Some(Handle::Centre);
    }
    let on_plane = |axis: &usize| {
        frame.plane_fade(*axis, style) > 0.0
            && frame
                .plane(*axis, style)
                .into_iter()
                .map(&project)
                .collect::<Option<Vec<_>>>()
                .is_some_and(|corners| inside(cursor, &corners))
    };
    nearest_axis(style.pick_radius, |axis| {
        if frame.axis_fade(axis, style) <= 0.0 {
            return None;
        }
        Some(segment_distance(
            cursor,
            project(frame.along(axis, style.centre))?,
            project(frame.along(axis, style.size))?,
        ))
    })
    .or_else(|| (0..3).find(on_plane).map(Handle::Plane))
    .or_else(|| {
        // A scale is uniform from anywhere else inside its ring.
        (mode == GizmoMode::Scale
            && from_centre <= style.outer_ring + style.pick_radius)
            .then_some(Handle::Centre)
    })
}

/// Finds the handle under each viewport's pointer, and draws the
/// gizmo sized for the viewport dragged in or the pointer is in.
fn handles(
    mut gizmos: Gizmos<HandleGizmos>,
    theme: Res<Theme<EditorTheme>>,
    settings: Res<GizmoSettings>,
    selected: Res<SelectedEntity>,
    active: Res<ActiveDrag>,
    buttons: Res<ButtonInput<MouseButton>>,
    nodes: Query<(Entity, &ViewportNode, &PointerLocation)>,
    mut cameras: Query<(
        &Camera,
        &GlobalTransform,
        &EditorCamera,
        &mut Hot,
    )>,
    targets: Query<&GlobalTransform, With<Transform>>,
    filled: Single<(&Mesh3d, &mut Visibility), With<HandleFills>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut drawn_in: Local<Option<Entity>>,
    mut drawn: Local<Fills>,
) {
    let style = &theme.0.gizmo;
    let target = selected.0.and_then(|aimed| targets.get(aimed).ok());

    let mut shown = None::<(u8, Entity, Shown)>;
    for (node, viewport, pointer) in &nodes {
        let Some((lens, view, orbit, mut hot)) = viewport
            .camera
            .and_then(|camera| cameras.get_mut(camera).ok())
        else {
            continue;
        };
        let frame = target
            .filter(|_| !orbit.follows_scene_camera())
            .and_then(|target| {
                Frame::of(target, *settings, lens, view)
            });
        let Some(mut frame) = frame else {
            hot.0 = None;
            continue;
        };

        let cursor = pointer
            .location
            .as_ref()
            .map(|location| location.position);
        let drag = active.0.as_ref().filter(|drag| drag.node == node);
        if let Some(drag) = drag {
            hot.0 = Some(drag.grab.handle);
            // The axes it took hold of, which a turn of the subject
            // would carry off with it.
            frame.axes = drag.grab.frame.axes;
        } else if !buttons.pressed(MouseButton::Left) {
            // Held through a press, so the drag that follows finds
            // the handle it pressed on.
            hot.0 = cursor.and_then(|cursor| {
                pick(settings.mode, &frame, cursor, style, |point| {
                    lens.world_to_viewport(view, point).ok()
                })
            });
        }

        // The viewport dragged in, else the one the pointer is in,
        // or failing that the one it was in last.
        let rank = if drag.is_some() {
            3
        } else if cursor.is_some() {
            2
        } else {
            u8::from(*drawn_in == Some(node))
        };
        if shown.as_ref().is_none_or(|(best, ..)| rank > *best) {
            let sweep = drag
                .filter(|drag| {
                    matches!(drag.edit, Pending::Rotation(_))
                })
                .and_then(|drag| drag.grab.sweep(drag.angle, style));
            let look = Shown {
                frame,
                hot: hot.0,
                dragging: drag.is_some(),
                sweep,
            };
            shown = Some((rank, node, look));
        }
    }

    let mut fills = Fills::default();
    if let Some((_, node, shown)) = shown {
        *drawn_in = Some(node);
        paint(&mut gizmos, &mut fills, settings.mode, &shown, style);
    }

    let (mesh, mut visibility) = filled.into_inner();
    let empty = fills.positions.is_empty();
    visibility.set_if_neq(if empty {
        Visibility::Hidden
    } else {
        Visibility::Visible
    });
    if !empty && *drawn != fills {
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            *mesh = fills.mesh();
        }
        *drawn = fills;
    }
}

#[cfg(test)]
mod tests;
