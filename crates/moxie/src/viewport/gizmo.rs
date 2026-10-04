//! The transform gizmo: handles drawn over the selection in a
//! viewport, dragged to move, turn or scale it.
//!
//! A drag is one [`Edit`] of a [`Transform`] field, so it writes the
//! way the inspector does and ends as one commit or one cancel.

use core::f32::consts::TAU;

use bevy::camera::visibility::RenderLayers;
use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::Theme;
use moxie_ui::inspector::{Edit, Field};
use moxie_ui::theme::{EditorTheme, GizmoStyle};

use super::{EDITOR_LAYER, EditorCamera};
use crate::SelectedEntity;
use crate::ui::text_field_focused;

/// Points a rotation ring is picked by.
const RING_STEPS: usize = 48;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<HandleGizmos>()
        .init_resource::<GizmoSettings>()
        .init_resource::<Aim>()
        .init_resource::<ActiveDrag>()
        .add_systems(Startup, style_handles)
        .add_systems(
            Update,
            (pick_mode.run_if(not(text_field_focused)), aim, drive)
                .chain()
                .after(super::place_cameras),
        )
        .add_systems(PostUpdate, handles.in_set(super::Overlay));
}

/// The gizmo's handles, drawn over the scene.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct HandleGizmos;

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

fn pick_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<GizmoSettings>,
) {
    let picked = [KeyCode::KeyW, KeyCode::KeyE, KeyCode::KeyR]
        .into_iter()
        .zip(GizmoMode::ALL)
        .find(|(key, _)| keys.just_pressed(*key));
    if let Some((_, mode)) = picked {
        settings.mode = mode;
    }
}

/// The subject the gizmo sits on. One an action drives is moved all
/// the same, until the timeline next writes it.
#[derive(Resource, Default, PartialEq)]
struct Aim(Option<Entity>);

fn aim(
    selected: Res<SelectedEntity>,
    placed: Query<(), With<Transform>>,
    mut aim: ResMut<Aim>,
) {
    let aimed = selected.0.filter(|&entity| placed.contains(entity));
    aim.set_if_neq(Aim(aimed));
}

/// One handle of the gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    /// Along or around the axis of this index.
    Axis(usize),
    /// At the origin: across the view for a translation, every axis
    /// at once for a scale.
    Centre,
}

/// The handle of a viewport's gizmo under its pointer. On the
/// viewport's camera.
#[derive(Component, Default)]
pub(crate) struct Hot(pub(crate) Option<Handle>);

/// Where a gizmo sits and how large it is, in the world.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: Vec3,
    axes: [Vec3; 3],
    /// A handle's length.
    size: f32,
}

impl Frame {
    /// The gizmo on `target`, a handle `style.size` long as `camera`
    /// draws it.
    fn of(
        target: &GlobalTransform,
        settings: GizmoSettings,
        camera: &Camera,
        view: &GlobalTransform,
        style: &GizmoStyle,
    ) -> Option<Self> {
        let origin = target.translation();
        let at = camera.world_to_viewport(view, origin).ok()?;
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
            size: style.size / per_unit,
        })
    }

    fn tip(&self, axis: usize) -> Vec3 {
        self.origin + self.axes[axis] * self.size
    }

    /// The rotation taking a shape drawn around z to around `axis`.
    fn facing(&self, axis: usize) -> Quat {
        Quat::from_rotation_arc(Vec3::Z, self.axes[axis])
    }

    /// Points around the ring of `axis`, the first one repeated last.
    fn ring(&self, axis: usize) -> impl Iterator<Item = Vec3> {
        let facing = self.facing(axis);
        let (origin, size) = (self.origin, self.size);
        (0..=RING_STEPS).map(move |step| {
            let angle = step as f32 / RING_STEPS as f32 * TAU;
            origin
                + facing
                    * Vec3::new(angle.cos(), angle.sin(), 0.0)
                    * size
        })
    }
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

/// The handle of `frame` under `cursor`, `project` taking a point in
/// the world to where the cursor's viewport shows it.
fn pick(
    mode: GizmoMode,
    frame: &Frame,
    cursor: Vec2,
    style: &GizmoStyle,
    project: impl Fn(Vec3) -> Option<Vec2>,
) -> Option<Handle> {
    let centre = project(frame.origin)?;
    if mode != GizmoMode::Rotate
        && cursor.distance(centre)
            <= style.centre.max(style.pick_radius)
    {
        return Some(Handle::Centre);
    }
    let distance = |axis: usize| match mode {
        GizmoMode::Rotate => {
            let ring = frame
                .ring(axis)
                .map(&project)
                .collect::<Option<Vec<_>>>()?;
            ring.windows(2)
                .map(|ends| {
                    segment_distance(cursor, ends[0], ends[1])
                })
                .min_by(f32::total_cmp)
        }
        GizmoMode::Translate | GizmoMode::Scale => {
            let tip = project(frame.tip(axis))?;
            Some(segment_distance(cursor, centre, tip))
        }
    };
    (0..3)
        .filter_map(|axis| Some((axis, distance(axis)?)))
        .filter(|(_, distance)| *distance <= style.pick_radius)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(axis, _)| Handle::Axis(axis))
}

/// A pointer over a viewport: where it is and the ray it casts.
#[derive(Clone, Copy, Debug)]
struct Pointing {
    cursor: Vec2,
    ray: Ray3d,
}

/// A handle as it was when a drag took hold of it.
#[derive(Clone, Copy, Debug)]
struct Grab {
    handle: Handle,
    frame: Frame,
    pointing: Pointing,
    /// The origin, where the viewport shows it.
    centre: Vec2,
    /// A handle's length there.
    reach: f32,
    /// The way the camera looks.
    view: Vec3,
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
    fn translation(
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
            Handle::Centre => {
                let from =
                    plane_hit(self.pointing.ray, origin, self.view)?;
                let to = plane_hit(now.ray, origin, self.view)?;
                Some((to - from).map(|part| snapped(part, snap)))
            }
        }
    }

    /// How far the drag has turned the subject, in the world.
    fn rotation(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Quat> {
        let Handle::Axis(axis) = self.handle else {
            return None;
        };
        let axis = self.frame.axes[axis];
        let from = self.pointing.cursor - self.centre;
        let to = now.cursor - self.centre;
        // Clockwise as drawn, a viewport's y running down.
        let swept = from.perp_dot(to).atan2(from.dot(to));
        // A turn about an axis is anticlockwise seen from its tip.
        let toward_camera = axis.dot(self.view) < 0.0;
        let angle = if toward_camera { -swept } else { swept };
        Some(Quat::from_axis_angle(axis, snapped(angle, snap)))
    }

    /// The factor the drag has scaled each of the subject's axes by.
    fn scale(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Vec3> {
        let Frame { origin, axes, size } = self.frame;
        match self.handle {
            Handle::Axis(axis) => {
                let from = axis_param(
                    self.pointing.ray,
                    origin,
                    axes[axis],
                )?;
                let to = axis_param(now.ray, origin, axes[axis])?;
                let factor = snapped(1.0 + (to - from) / size, snap);
                let mut scale = Vec3::ONE;
                scale[axis] = factor;
                Some(scale)
            }
            Handle::Centre => {
                let from = self.pointing.cursor.distance(self.centre);
                let to = now.cursor.distance(self.centre);
                // Grabbed on the origin itself, so measured in
                // handle lengths: no distance from it is a factor
                // of the first.
                let factor = 1.0 + (to - from) / self.reach;
                Some(Vec3::splat(snapped(factor, snap)))
            }
        }
    }
}

/// The translation of a subject at `start` under `parent`, moved by
/// `delta` in the world.
fn moved(
    start: &Transform,
    parent: &GlobalTransform,
    delta: Vec3,
) -> Vec3 {
    start.translation
        + parent.affine().inverse().transform_vector3(delta)
}

/// The rotation of a subject at `start` under `parent`, turned by
/// `delta` in the world.
fn turned(
    start: &Transform,
    parent: &GlobalTransform,
    delta: Quat,
) -> Quat {
    let parent = parent.rotation();
    (parent.inverse() * delta * parent * start.rotation).normalize()
}

/// The field a drag edits.
enum Pending {
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

struct Drag {
    /// The viewport dragged in.
    node: Entity,
    grab: Grab,
    edit: Pending,
    /// The subject's own transform when the drag began.
    start: Transform,
    /// Its parent's, in the world.
    parent: GlobalTransform,
}

/// The gizmo drag under way, of which there is one at most.
#[derive(Resource, Default)]
struct ActiveDrag(Option<Drag>);

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
    let aimed = world.resource::<Aim>().0?;
    let camera = world.get::<ViewportNode>(node)?.camera?;
    let handle = world.get::<Hot>(camera)?.0?;
    let view = world.get::<GlobalTransform>(camera)?;
    let lens = world.get::<Camera>(camera)?;
    let target = world.get::<GlobalTransform>(aimed)?;
    let frame = Frame::of(target, settings, lens, view, style)?;

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
            view: view.rotation() * Vec3::NEG_Z,
        },
        edit,
        start: *world.get::<Transform>(aimed)?,
        parent,
    })
}

/// Lets a drag on the viewport `node` take hold of the gizmo.
pub(super) fn watch(node: &mut EntityWorldMut) {
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
        && !super::alt(&keys)
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
fn drive(world: &mut World) {
    let Some(drag) = world.resource_mut::<ActiveDrag>().0.take()
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
                if let Some(delta) = grab.rotation(&now, snap) {
                    edit.write(world, turned(start, parent, delta));
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
    world.resource_mut::<ActiveDrag>().0 = Some(drag);
}

/// Finds the handle under each viewport's pointer, and draws the
/// gizmo sized for the viewport the pointer is in.
fn handles(
    mut gizmos: Gizmos<HandleGizmos>,
    theme: Res<Theme<EditorTheme>>,
    settings: Res<GizmoSettings>,
    aim: Res<Aim>,
    active: Res<ActiveDrag>,
    buttons: Res<ButtonInput<MouseButton>>,
    nodes: Query<(Entity, &ViewportNode, &PointerLocation)>,
    mut cameras: Query<(
        &Camera,
        &GlobalTransform,
        &EditorCamera,
        &mut Hot,
    )>,
    targets: Query<&GlobalTransform>,
    mut drawn_in: Local<Option<Entity>>,
) {
    let style = &theme.0.gizmo;
    let target = aim.0.and_then(|aimed| targets.get(aimed).ok());

    let mut shown = None::<(u8, Entity, Frame, Option<Handle>, Quat)>;
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
                Frame::of(target, *settings, lens, view, style)
            });
        let Some(frame) = frame else {
            hot.0 = None;
            continue;
        };

        let cursor = pointer
            .location
            .as_ref()
            .map(|location| location.position);
        let dragged = active
            .0
            .as_ref()
            .filter(|drag| drag.node == node)
            .map(|drag| drag.grab.handle);
        if dragged.is_some() {
            hot.0 = dragged;
        } else if !buttons.pressed(MouseButton::Left) {
            // Held through a press, so the drag that follows finds
            // the handle it pressed on.
            hot.0 = cursor.and_then(|cursor| {
                pick(settings.mode, &frame, cursor, style, |point| {
                    lens.world_to_viewport(view, point).ok()
                })
            });
        }

        // The viewport the pointer is in, or failing that the one it
        // was in last.
        let rank = if cursor.is_some() {
            2
        } else {
            u8::from(*drawn_in == Some(node))
        };
        if shown.as_ref().is_none_or(|(best, ..)| rank > *best) {
            shown = Some((rank, node, frame, hot.0, view.rotation()));
        }
    }

    let Some((_, node, frame, hot, facing)) = shown else {
        return;
    };
    *drawn_in = Some(node);
    let color = |handle: Handle, plain: Color| {
        if hot == Some(handle) {
            style.hot
        } else {
            plain
        }
    };
    let to_world = frame.size / style.size;
    for (axis, plain) in style.axes.into_iter().enumerate() {
        let color = color(Handle::Axis(axis), plain);
        match settings.mode {
            GizmoMode::Translate => {
                gizmos
                    .arrow(frame.origin, frame.tip(axis), color)
                    .with_tip_length(style.tip * 2.0 * to_world);
            }
            GizmoMode::Rotate => {
                gizmos
                    .circle(
                        Isometry3d::new(
                            frame.origin,
                            frame.facing(axis),
                        ),
                        frame.size,
                        color,
                    )
                    .resolution(RING_STEPS as u32);
            }
            GizmoMode::Scale => {
                gizmos.line(frame.origin, frame.tip(axis), color);
                gizmos.cube(
                    Transform::from_translation(frame.tip(axis))
                        .with_rotation(frame.facing(axis))
                        .with_scale(Vec3::splat(
                            style.tip * to_world,
                        )),
                    color,
                );
            }
        }
    }
    if settings.mode != GizmoMode::Rotate {
        gizmos.circle(
            Isometry3d::new(frame.origin, facing),
            style.centre * to_world,
            color(Handle::Centre, style.centre_color),
        );
    }
}

#[cfg(test)]
mod tests {
    use core::f32::consts::FRAC_PI_2;

    use super::*;

    /// A camera at +z looking down -z, drawing one world unit as
    /// `ZOOM` pixels around the middle of its viewport.
    const ZOOM: f32 = 100.0;
    const MIDDLE: Vec2 = Vec2::new(400.0, 300.0);

    fn project(point: Vec3) -> Option<Vec2> {
        Some(MIDDLE + Vec2::new(point.x, -point.y) * ZOOM)
    }

    fn point_at(cursor: Vec2) -> Pointing {
        let offset = (cursor - MIDDLE) / ZOOM;
        Pointing {
            cursor,
            ray: Ray3d::new(
                Vec3::new(offset.x, -offset.y, 10.0),
                Dir3::NEG_Z,
            ),
        }
    }

    fn frame() -> Frame {
        Frame {
            origin: Vec3::ZERO,
            axes: Vec3::AXES,
            size: 1.0,
        }
    }

    fn grab(handle: Handle, cursor: Vec2) -> Grab {
        Grab {
            handle,
            frame: frame(),
            pointing: point_at(cursor),
            centre: MIDDLE,
            reach: ZOOM,
            view: Vec3::NEG_Z,
        }
    }

    fn style() -> GizmoStyle {
        EditorTheme::default().gizmo
    }

    #[test]
    fn the_pointer_picks_the_handle_it_is_on() {
        let pick = |mode, cursor| {
            pick(mode, &frame(), cursor, &style(), project)
        };
        let along_x = MIDDLE + Vec2::new(60.0, 2.0);
        let along_y = MIDDLE + Vec2::new(-2.0, -60.0);

        assert_eq!(
            pick(GizmoMode::Translate, along_x),
            Some(Handle::Axis(0))
        );
        assert_eq!(
            pick(GizmoMode::Scale, along_y),
            Some(Handle::Axis(1))
        );
        assert_eq!(
            pick(GizmoMode::Translate, MIDDLE),
            Some(Handle::Centre)
        );
        assert_eq!(
            pick(GizmoMode::Translate, MIDDLE + Vec2::splat(60.0)),
            None
        );
        // The ring around z faces this camera: a circle a handle's
        // length out.
        assert_eq!(
            pick(
                GizmoMode::Rotate,
                MIDDLE + Vec2::from_angle(1.0) * ZOOM
            ),
            Some(Handle::Axis(2))
        );
        // The other two are edge on, and cross at the origin.
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + Vec2::new(0.0, 50.0)),
            Some(Handle::Axis(0))
        );
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + Vec2::splat(40.0)),
            None
        );
    }

    #[test]
    fn an_axis_drag_moves_along_its_axis_only() {
        let from = MIDDLE + Vec2::new(50.0, 0.0);
        let grab = grab(Handle::Axis(0), from);
        let now = point_at(from + Vec2::new(150.0, 80.0));

        let delta = grab.translation(&now, None).expect("it moves");
        assert!(delta.abs_diff_eq(Vec3::new(1.5, 0.0, 0.0), 1e-4));

        let snapped =
            grab.translation(&now, Some(1.0)).expect("it moves");
        assert!(snapped.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-4));
    }

    #[test]
    fn a_centre_drag_moves_across_the_view() {
        let grab = grab(Handle::Centre, MIDDLE);
        let now = point_at(MIDDLE + Vec2::new(100.0, -200.0));

        let delta = grab.translation(&now, None).expect("it moves");
        assert!(delta.abs_diff_eq(Vec3::new(1.0, 2.0, 0.0), 1e-4));
    }

    #[test]
    fn a_ring_drag_turns_the_way_the_pointer_goes() {
        // From the right of the origin to above it, as drawn.
        let grab =
            grab(Handle::Axis(2), MIDDLE + Vec2::new(100.0, 0.0));
        let now = point_at(MIDDLE + Vec2::new(0.0, -100.0));

        let turn = grab.rotation(&now, None).expect("it turns");
        assert!((turn * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));

        // Seen from behind, the same sweep turns the other way.
        let behind = Grab {
            view: Vec3::Z,
            ..grab
        };
        let turn = behind.rotation(&now, None).expect("it turns");
        assert!((turn * Vec3::X).abs_diff_eq(Vec3::NEG_Y, 1e-4));

        let near = point_at(MIDDLE + Vec2::new(20.0, -100.0));
        let snap = Some(FRAC_PI_2);
        let turn = grab.rotation(&near, snap).expect("it turns");
        assert!((turn * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));
    }

    #[test]
    fn a_scale_drag_grows_one_axis_or_all_of_them() {
        let from = MIDDLE + Vec2::new(0.0, -100.0);
        let axis = grab(Handle::Axis(1), from);
        let now = point_at(from + Vec2::new(30.0, -50.0));
        let factor = axis.scale(&now, None).expect("it scales");
        assert!(factor.abs_diff_eq(Vec3::new(1.0, 1.5, 1.0), 1e-4));

        let from = MIDDLE + Vec2::new(4.0, 0.0);
        let centre = grab(Handle::Centre, from);
        let now = point_at(from + Vec2::new(ZOOM, 0.0));
        let factor = centre.scale(&now, None).expect("it scales");
        assert!(factor.abs_diff_eq(Vec3::splat(2.0), 1e-4));
    }

    #[test]
    fn a_world_move_lands_in_the_parents_space() {
        let parent = GlobalTransform::from(
            Transform::from_rotation(Quat::from_rotation_y(
                FRAC_PI_2,
            ))
            .with_scale(Vec3::splat(2.0)),
        );
        let start = Transform::from_xyz(1.0, 0.0, 0.0);
        let delta = Vec3::new(0.0, 0.0, -4.0);

        let moved = moved(&start, &parent, delta);
        let before = parent.transform_point(start.translation);
        let after = parent.transform_point(moved);
        assert!((after - before).abs_diff_eq(delta, 1e-4));
    }

    #[test]
    fn a_world_turn_lands_in_the_parents_space() {
        let parent = GlobalTransform::from(Transform::from_rotation(
            Quat::from_rotation_y(FRAC_PI_2),
        ));
        let start =
            Transform::from_rotation(Quat::from_rotation_x(0.3));
        let delta = Quat::from_rotation_z(0.7);

        let turned = turned(&start, &parent, delta);
        let before = parent.rotation() * start.rotation;
        let after = parent.rotation() * turned;
        assert!(after.abs_diff_eq(delta * before, 1e-4));
    }
}
