//! Dragging a handle: one [`Edit`] of a [`Transform`] field, written
//! the way the inspector writes and ended as a commit or a cancel.

use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::Theme;
use moxie_ui::SelectedEntity;
use moxie_ui::inspector::Edit;
use moxie_ui::theme::{EditorTheme, GizmoStyle};

use super::{Arc, Frame, GizmoMode, GizmoSettings, Handle, Hot};

/// A pointer over a viewport: where it is and the ray it casts.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pointing {
    pub(super) cursor: Vec2,
    pub(super) ray: Ray3d,
}

/// A handle as it was when a drag took hold of it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Grab {
    pub(super) handle: Handle,
    pub(super) frame: Frame,
    pub(super) pointing: Pointing,
    /// The origin, where the viewport shows it.
    pub(super) centre: Vec2,
    /// A handle's length there.
    pub(super) reach: f32,
}

/// How far along the line through `origin` in the unit direction
/// `axis` the point nearest to `ray` is. `None` when the two are
/// parallel.
fn axis_param(ray: Ray3d, origin: Vec3, axis: Vec3) -> Option<f32> {
    let direction = *ray.direction;
    let between = origin - ray.origin;
    let lean = axis.dot(direction);
    let denominator = 1.0 - lean * lean;
    if denominator <= 1e-6 {
        return None;
    }
    Some(
        (lean * direction.dot(between) - axis.dot(between))
            / denominator,
    )
}

/// Where `ray` meets the plane through `origin` facing `normal`.
fn plane_hit(ray: Ray3d, origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let denominator = normal.dot(*ray.direction);
    if denominator.abs() <= 1e-6 {
        return None;
    }
    let distance = (origin - ray.origin).dot(normal) / denominator;
    Some(ray.origin + *ray.direction * distance)
}

/// `value` to the nearest multiple of `step`, when there is one.
fn snapped(value: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) if step > 0.0 => (value / step).round() * step,
        _ => value,
    }
}

impl Grab {
    /// How far the drag has moved the subject, in the world.
    pub(super) fn translation(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Vec3> {
        let Frame { origin, axes, .. } = self.frame;
        match self.handle {
            Handle::Axis(axis) => {
                let axis = axes[axis];
                let from =
                    axis_param(self.pointing.ray, origin, axis)?;
                let to = axis_param(now.ray, origin, axis)?;
                Some(axis * snapped(to - from, snap))
            }
            Handle::Plane(axis) => {
                let normal = axes[axis];
                let from =
                    plane_hit(self.pointing.ray, origin, normal)?;
                let to = plane_hit(now.ray, origin, normal)?;
                let [a, b] = self.frame.sides(axis);
                let along =
                    |side: Vec3| snapped((to - from).dot(side), snap);
                Some(a * along(a) + b * along(b))
            }
            Handle::Centre => {
                let normal = self.frame.facing * Vec3::Z;
                let from =
                    plane_hit(self.pointing.ray, origin, normal)?;
                let to = plane_hit(now.ray, origin, normal)?;
                Some((to - from).map(|part| snapped(part, snap)))
            }
            Handle::View | Handle::Ball => None,
        }
    }

    /// The axis the drag has turned the subject about and the angle
    /// it has turned it by, in the world.
    pub(super) fn turn(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<(Vec3, f32)> {
        let axis = match self.handle {
            Handle::Axis(axis) => self.frame.axes[axis],
            Handle::View => -self.frame.look,
            Handle::Ball => {
                let rolled = now.cursor - self.pointing.cursor;
                // The way the near side of a ball goes under the
                // pointer, a handle's length of it turning a radian.
                let axis = self.frame.facing
                    * Vec3::new(rolled.y, rolled.x, 0.0);
                let angle = rolled.length() / self.reach;
                return Some((
                    axis.try_normalize()?,
                    snapped(angle, snap),
                ));
            }
            Handle::Plane(_) | Handle::Centre => return None,
        };
        let from = self.pointing.cursor - self.centre;
        let to = now.cursor - self.centre;
        // Clockwise as drawn, a viewport's y running down.
        let swept = from.perp_dot(to).atan2(from.dot(to));
        // A turn about an axis is anticlockwise seen from its tip.
        let toward_viewer = axis.dot(self.frame.look) < 0.0;
        let angle = if toward_viewer { -swept } else { swept };
        Some((axis, snapped(angle, snap)))
    }

    /// The factor the drag has scaled each of the subject's axes by.
    pub(super) fn scale(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Vec3> {
        let Frame { origin, axes, .. } = self.frame;
        // No distance from the origin is a factor of another when
        // one is next to nothing, so each is measured in handle
        // lengths.
        let spread = || {
            let from = self.pointing.cursor.distance(self.centre);
            let to = now.cursor.distance(self.centre);
            snapped(1.0 + (to - from) / self.reach, snap)
        };
        match self.handle {
            Handle::Axis(axis) => {
                let from = axis_param(
                    self.pointing.ray,
                    origin,
                    axes[axis],
                )?;
                let to = axis_param(now.ray, origin, axes[axis])?;
                let length = self.reach * self.frame.scale;
                let mut scale = Vec3::ONE;
                scale[axis] =
                    snapped(1.0 + (to - from) / length, snap);
                Some(scale)
            }
            Handle::Plane(axis) => {
                let mut scale = Vec3::splat(spread());
                scale[axis] = 1.0;
                Some(scale)
            }
            Handle::Centre => Some(Vec3::splat(spread())),
            Handle::View | Handle::Ball => None,
        }
    }

    /// The arc a turn by `angle` has swept, and the radius of the
    /// ring it is on.
    pub(super) fn sweep(
        &self,
        angle: f32,
        style: &GizmoStyle,
    ) -> Option<(Arc, f32)> {
        let Frame { origin, look, .. } = self.frame;
        let (normal, radius) = match self.handle {
            Handle::Axis(axis) => (self.frame.axes[axis], style.size),
            Handle::View => (-look, style.outer_ring),
            _ => return None,
        };
        // From where the pointer took hold of the ring's plane, or
        // from the nearest of the ring when that is edge on.
        let start = plane_hit(self.pointing.ray, origin, normal)
            .and_then(|hit| (hit - origin).try_normalize())
            .unwrap_or_else(|| Arc::facing(normal, look, 0.0).start);
        let arc = Arc {
            start,
            quarter: normal.cross(start),
            from: 0.0,
            to: angle,
        };
        Some((arc, radius))
    }
}

/// The translation of a subject at `start` under `parent`, moved by
/// `delta` in the world.
pub(super) fn moved(
    start: &Transform,
    parent: &GlobalTransform,
    delta: Vec3,
) -> Vec3 {
    start.translation
        + parent.affine().inverse().transform_vector3(delta)
}

/// The rotation of a subject at `start` under `parent`, turned by
/// `delta` in the world.
pub(super) fn turned(
    start: &Transform,
    parent: &GlobalTransform,
    delta: Quat,
) -> Quat {
    let parent = parent.rotation();
    (parent.inverse() * delta * parent * start.rotation).normalize()
}

/// The field a drag edits.
pub(super) enum Pending {
    Translation(Edit<Vec3>),
    Rotation(Edit<Quat>),
    Scale(Edit<Vec3>),
}

impl Pending {
    fn commit(self) {
        match self {
            Self::Translation(edit) | Self::Scale(edit) => {
                edit.commit();
            }
            Self::Rotation(edit) => edit.commit(),
        }
    }

    fn cancel(self, world: &mut World) {
        match self {
            Self::Translation(edit) | Self::Scale(edit) => {
                edit.cancel(world);
            }
            Self::Rotation(edit) => edit.cancel(world),
        }
    }
}

pub(super) struct Drag {
    /// The viewport dragged in.
    pub(super) node: Entity,
    pub(super) grab: Grab,
    pub(super) edit: Pending,
    /// The subject's own transform when the drag began.
    start: Transform,
    /// Its parent's, in the world.
    parent: GlobalTransform,
    /// The angle a rotation has turned it by.
    pub(super) angle: f32,
}

/// The gizmo drag under way, of which there is one at most.
#[derive(Resource, Default)]
pub(super) struct ActiveDrag(pub(super) Option<Drag>);

/// The pointer over the viewport `node`, through its `camera`.
fn pointing(
    world: &World,
    node: Entity,
    camera: Entity,
) -> Option<Pointing> {
    let cursor = world
        .get::<PointerLocation>(node)?
        .location
        .as_ref()?
        .position;
    let ray = world
        .get::<Camera>(camera)?
        .viewport_to_world(
            world.get::<GlobalTransform>(camera)?,
            cursor,
        )
        .ok()?;
    Some(Pointing { cursor, ray })
}

/// The drag of the hot handle of the viewport `node`.
fn grab(world: &World, node: Entity) -> Option<Drag> {
    let settings = *world.resource::<GizmoSettings>();
    let style = &world.resource::<Theme<EditorTheme>>().0.gizmo;
    // One an action drives is moved all the same, until the
    // timeline next writes it.
    let aimed = world.resource::<SelectedEntity>().0?;
    let camera = world.get::<ViewportNode>(node)?.camera?;
    let handle = world.get::<Hot>(camera)?.0?;
    let view = world.get::<GlobalTransform>(camera)?;
    let lens = world.get::<Camera>(camera)?;
    let target = world.get::<GlobalTransform>(aimed)?;
    let frame = Frame::of(target, settings, lens, view)?;

    let field = settings.mode.field(aimed);
    let edit = match settings.mode {
        GizmoMode::Translate => {
            Pending::Translation(Edit::begin(world, field)?)
        }
        GizmoMode::Rotate => {
            Pending::Rotation(Edit::begin(world, field)?)
        }
        GizmoMode::Scale => {
            Pending::Scale(Edit::begin(world, field)?)
        }
    };
    let parent = world
        .get::<ChildOf>(aimed)
        .and_then(|child| {
            world.get::<GlobalTransform>(child.parent())
        })
        .copied()
        .unwrap_or_default();
    Some(Drag {
        node,
        grab: Grab {
            handle,
            frame,
            pointing: pointing(world, node, camera)?,
            centre: lens
                .world_to_viewport(view, frame.origin)
                .ok()?,
            reach: style.size,
        },
        edit,
        start: *world.get::<Transform>(aimed)?,
        parent,
        angle: 0.0,
    })
}

/// Lets a drag on the viewport `node` take hold of the gizmo.
pub(crate) fn watch(node: &mut EntityWorldMut) {
    node.observe(on_drag_start).observe(on_drag_end);
}

fn on_drag_start(
    mut start: On<Pointer<DragStart>>,
    hot: Query<&Hot>,
    nodes: Query<&ViewportNode>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let node = start.event_target();
    // With alt held the drag is the camera's.
    let grabs = start.button == PointerButton::Primary
        && !crate::alt(&keys)
        && nodes
            .get(node)
            .ok()
            .and_then(|node| node.camera)
            .and_then(|camera| hot.get(camera).ok())
            .is_some_and(|hot| hot.0.is_some());
    if !grabs {
        return;
    }
    start.propagate(false);
    commands.queue(move |world: &mut World| {
        let drag = grab(world, node);
        world.resource_mut::<ActiveDrag>().0 = drag;
    });
}

fn on_drag_end(
    end: On<Pointer<DragEnd>>,
    mut active: ResMut<ActiveDrag>,
) {
    let ended = end.button == PointerButton::Primary
        && active
            .0
            .as_ref()
            .is_some_and(|drag| drag.node == end.event_target());
    if !ended {
        return;
    }
    if let Some(drag) = active.0.take() {
        drag.edit.commit();
    }
}

/// Writes the drag under way, or on Escape puts the subject back.
pub(super) fn drive(world: &mut World) {
    let Some(mut drag) = world.resource_mut::<ActiveDrag>().0.take()
    else {
        return;
    };
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let cancels = keys.just_pressed(KeyCode::Escape);
    let snaps = keys
        .any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if cancels {
        drag.edit.cancel(world);
        return;
    }
    let style = world.resource::<Theme<EditorTheme>>().0.gizmo;
    let Some(camera) = world
        .get::<ViewportNode>(drag.node)
        .and_then(|node| node.camera)
    else {
        // The viewport closed under the drag, which no release will
        // end.
        drag.edit.commit();
        return;
    };

    let mut swept = drag.angle;
    if let Some(now) = pointing(world, drag.node, camera) {
        let Drag {
            grab,
            start,
            parent,
            ..
        } = &drag;
        match &drag.edit {
            Pending::Translation(edit) => {
                let snap = snaps.then_some(style.translate_snap);
                if let Some(delta) = grab.translation(&now, snap) {
                    edit.write(world, moved(start, parent, delta));
                }
            }
            Pending::Rotation(edit) => {
                let snap = snaps.then_some(style.rotate_snap);
                if let Some((axis, angle)) = grab.turn(&now, snap) {
                    let delta = Quat::from_axis_angle(axis, angle);
                    edit.write(world, turned(start, parent, delta));
                    swept = angle;
                }
            }
            Pending::Scale(edit) => {
                let snap = snaps.then_some(style.scale_snap);
                if let Some(factor) = grab.scale(&now, snap) {
                    edit.write(world, start.scale * factor);
                }
            }
        }
    }
    drag.angle = swept;
    world.resource_mut::<ActiveDrag>().0 = Some(drag);
}
