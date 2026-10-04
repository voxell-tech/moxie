//! The transform gizmo: handles drawn over the selection in a
//! viewport, dragged to move, turn or scale it.
//!
//! A drag is one [`Edit`] of a [`Transform`] field, so it writes the
//! way the inspector does and ends as one commit or one cancel.

use core::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{
    NoFrustumCulling, RenderLayers, VisibilitySystems,
};
use bevy::light::NotShadowCaster;
use bevy::mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey,
    MaterialExtensionPipeline,
};
use bevy::picking::events::{DragEnd, DragStart, Pointer};
use bevy::picking::pointer::{PointerButton, PointerLocation};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::ui::widget::ViewportNode;
use bevy_fynix::Theme;
use moxie_ui::inspector::{Edit, Field};
use moxie_ui::theme::{EditorTheme, GizmoStyle};
use moxie_ui::{SelectedEntity, text_field_focused};

use super::{EDITOR_LAYER, EditorCamera};

/// Segments a rotation ring is drawn and picked by.
const RING_STEPS: usize = 48;
/// Sides of the cone that tips a translation handle.
const CONE_SIDES: usize = 12;

/// The material of the gizmo's filled shapes.
type FillMaterial = ExtendedMaterial<StandardMaterial, OnTop>;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<HandleGizmos>()
        .add_plugins(MaterialPlugin::<FillMaterial>::default())
        .init_resource::<GizmoSettings>()
        .init_resource::<Aim>()
        .init_resource::<ActiveDrag>()
        .add_systems(Startup, (style_handles, spawn_fills))
        .add_systems(
            Update,
            (pick_mode.run_if(not(text_field_focused)), aim, drive)
                .chain()
                .after(super::place_cameras),
        )
        .add_systems(
            PostUpdate,
            handles
                .in_set(super::Overlay)
                .before(VisibilitySystems::VisibilityPropagate),
        );
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

/// Draws a [`StandardMaterial`] over whatever is in front of it.
#[derive(Asset, AsBindGroup, TypePath, Clone, Default)]
struct OnTop {}

impl MaterialExtension for OnTop {
    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_compare = Some(CompareFunction::Always);
        }
        // The theme's colours as they are, like the lines beside
        // them.
        if let Some(fragment) = &mut descriptor.fragment {
            fragment
                .shader_defs
                .retain(|def| *def != "TONEMAP_IN_SHADER".into());
        }
        Ok(())
    }
}

/// Marker component for the mesh of the gizmo's filled shapes.
#[derive(Component)]
struct HandleFills;

fn spawn_fills(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FillMaterial>>,
) {
    commands.spawn((
        HandleFills,
        Mesh3d(meshes.add(Fills::default().mesh())),
        MeshMaterial3d(materials.add(FillMaterial {
            base: StandardMaterial {
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            },
            extension: OnTop {},
        })),
        RenderLayers::layer(EDITOR_LAYER),
        Visibility::Hidden,
        // Its triangles are in the world, wherever the subject is.
        NoFrustumCulling,
        NotShadowCaster,
        Pickable::IGNORE,
    ));
}

/// The triangles of the gizmo's filled shapes, in the world. Only
/// the side a triangle winds anticlockwise from is drawn.
#[derive(Default, PartialEq)]
struct Fills {
    positions: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
}

impl Fills {
    fn triangle(&mut self, corners: [Vec3; 3], color: Color) {
        self.positions.extend(corners);
        self.colors.extend([color.to_linear().to_f32_array(); 3]);
    }

    fn quad(&mut self, [a, b, c, d]: [Vec3; 4], color: Color) {
        self.triangle([a, b, c], color);
        self.triangle([a, c, d], color);
    }

    /// A flat fan from `hub` out to each pair of points along `rim`,
    /// seen from both sides.
    fn fan(
        &mut self,
        hub: Vec3,
        rim: impl IntoIterator<Item = Vec3>,
        color: Color,
    ) {
        let rim = rim.into_iter().collect::<Vec<_>>();
        for ends in rim.windows(2) {
            self.triangle([hub, ends[0], ends[1]], color);
            self.triangle([hub, ends[1], ends[0]], color);
        }
    }

    /// A cone from a base of `radius` around `base` up to `apex`.
    fn cone(
        &mut self,
        base: Vec3,
        apex: Vec3,
        radius: f32,
        color: Color,
    ) {
        let Some(axis) = (apex - base).try_normalize() else {
            return;
        };
        let rim = Arc::full(axis)
            .points(base, radius, CONE_SIDES)
            .collect::<Vec<_>>();
        for ends in rim.windows(2) {
            self.triangle([apex, ends[0], ends[1]], color);
            self.triangle([base, ends[1], ends[0]], color);
        }
    }

    /// A box around `centre`, reaching each of `halves` either way.
    fn cuboid(
        &mut self,
        centre: Vec3,
        halves: [Vec3; 3],
        color: Color,
    ) {
        for side in 0..3 {
            let out = halves[side];
            let across = halves[(side + 1) % 3];
            let up = halves[(side + 2) % 3];
            let face = |out: Vec3, across: Vec3| {
                [
                    centre + out - across - up,
                    centre + out + across - up,
                    centre + out + across + up,
                    centre + out - across + up,
                ]
            };
            self.quad(face(out, across), color);
            self.quad(face(-out, -across), color);
        }
    }

    fn mesh(&self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            self.positions.clone(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_COLOR,
            self.colors.clone(),
        )
    }
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
    fn turn(
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
    fn scale(
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
    fn sweep(
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
    /// The angle a rotation has turned it by.
    angle: f32,
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

/// Finds the handle under each viewport's pointer, and draws the
/// gizmo sized for the viewport dragged in or the pointer is in.
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
    filled: Single<(&Mesh3d, &mut Visibility), With<HandleFills>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut drawn_in: Local<Option<Entity>>,
    mut drawn: Local<Fills>,
) {
    let style = &theme.0.gizmo;
    let target = aim.0.and_then(|aimed| targets.get(aimed).ok());

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

/// The gizmo as a viewport shows it.
struct Shown {
    frame: Frame,
    /// The handle under the pointer, or dragged.
    hot: Option<Handle>,
    dragging: bool,
    /// The arc a dragged ring has turned through, and its radius.
    sweep: Option<(Arc, f32)>,
}

/// Draws the handles of `mode`: all of them with the hot one
/// brighter, or the dragged one and what a drag keeps beside it.
fn paint(
    gizmos: &mut Gizmos<HandleGizmos>,
    fills: &mut Fills,
    mode: GizmoMode,
    shown: &Shown,
    style: &GizmoStyle,
) {
    let Shown {
        frame,
        hot,
        dragging,
        sweep,
    } = *shown;
    let toward = -frame.look;
    let plain = |handle: Handle| match handle {
        Handle::Axis(axis) | Handle::Plane(axis) => style.axes[axis],
        Handle::Centre | Handle::View | Handle::Ball => {
            style.centre_color
        }
    };
    let tint = |handle: Handle, fade: f32| {
        let alpha = if hot == Some(handle) {
            style.hot_alpha
        } else {
            style.rest_alpha
        };
        let color = plain(handle);
        color.with_alpha(color.alpha() * alpha * fade)
    };
    let held = |handle: Handle| dragging && hot == Some(handle);
    let dropped = |handle: Handle| dragging && hot != Some(handle);

    if mode == GizmoMode::Rotate {
        let rings = (0..3)
            .map(|axis| {
                let arc = if dragging {
                    Arc::full(frame.axes[axis])
                } else {
                    frame.arc(axis, style)
                };
                (Handle::Axis(axis), arc, style.size)
            })
            .chain([(
                Handle::View,
                Arc::full(toward),
                style.outer_ring,
            )]);
        for (handle, arc, pixels) in rings {
            if !dropped(handle) {
                gizmos.linestrip(
                    frame.ring(arc, pixels),
                    tint(handle, 1.0),
                );
            }
        }
        if let Some((arc, pixels)) = sweep {
            let rim = frame.ring(arc, pixels).collect::<Vec<_>>();
            let edge = hot.map_or(style.centre_color, plain);
            for end in [rim[0], rim[RING_STEPS]] {
                gizmos.line(frame.origin, end, edge);
            }
            fills.fan(frame.origin, rim, style.sweep);
        }
        if hot == Some(Handle::Ball) {
            let color = plain(Handle::Ball);
            fills.fan(
                frame.origin,
                frame.ring(Arc::full(toward), style.size),
                color.with_alpha(color.alpha() * style.ball_alpha),
            );
        }
        return;
    }

    for axis in 0..3 {
        let handle = Handle::Axis(axis);
        let fade = frame.axis_fade(axis, style);
        // A translation keeps the other axes to move against.
        let bare = dropped(handle);
        if fade <= 0.0 || (bare && mode == GizmoMode::Scale) {
            continue;
        }
        let color = tint(handle, fade);
        let from = if held(handle) {
            frame.origin
        } else {
            frame.along(axis, style.centre)
        };
        if bare {
            gizmos.line(from, frame.along(axis, style.size), color);
        } else if mode == GizmoMode::Translate {
            let neck =
                frame.along(axis, style.size - style.cone_length);
            gizmos.line(from, neck, color);
            fills.cone(
                neck,
                frame.along(axis, style.size),
                style.cone_radius * frame.scale,
                color,
            );
        } else {
            let half = style.tip / 2.0;
            gizmos.line(
                from,
                frame.along(axis, style.size - style.tip),
                color,
            );
            fills.cuboid(
                frame.along(axis, style.size - half),
                frame.axes.map(|axis| axis * half * frame.scale),
                color,
            );
        }
    }
    for axis in 0..3 {
        let handle = Handle::Plane(axis);
        let fade = frame.plane_fade(axis, style);
        if fade <= 0.0 || dropped(handle) {
            continue;
        }
        let corners = frame.plane(axis, style);
        let color = tint(handle, fade);
        gizmos.linestrip(
            corners.into_iter().chain([corners[0]]),
            color,
        );
        fills.fan(
            corners[0],
            [corners[1], corners[2], corners[3]],
            color.with_alpha(color.alpha() * style.plane_fill),
        );
    }
    if !dropped(Handle::Centre) {
        let color = tint(Handle::Centre, 1.0);
        let mut ring = |pixels: f32| {
            gizmos.linestrip(
                frame.ring(Arc::full(toward), pixels),
                color,
            );
        };
        ring(style.centre);
        if mode == GizmoMode::Scale {
            ring(style.outer_ring);
        }
    }
}

#[cfg(test)]
mod tests {
    use core::f32::consts::{FRAC_PI_2, FRAC_PI_6};

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
            scale: 1.0 / ZOOM,
            look: Vec3::NEG_Z,
            facing: Quat::IDENTITY,
        }
    }

    /// The point `pixels` out on the unit circle point `on` of a
    /// frame with `axes`, where the viewport shows it.
    fn shown_at(axes: Quat, on: Vec3, pixels: f32) -> Vec2 {
        MIDDLE
            + (axes * on * pixels).truncate() * Vec2::new(1.0, -1.0)
    }

    fn turn(grab: &Grab, now: &Pointing, snap: Option<f32>) -> Quat {
        let (axis, angle) = grab.turn(now, snap).expect("it turns");
        Quat::from_axis_angle(axis, angle)
    }

    fn grab(handle: Handle, cursor: Vec2) -> Grab {
        Grab {
            handle,
            frame: frame(),
            pointing: point_at(cursor),
            centre: MIDDLE,
            reach: ZOOM,
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
        let between = Vec2::new(60.0, 60.0);
        assert_eq!(
            pick(GizmoMode::Translate, MIDDLE + between),
            None
        );
        // Inside its ring, a scale is uniform from anywhere.
        assert_eq!(
            pick(GizmoMode::Scale, MIDDLE + between),
            Some(Handle::Centre)
        );
        // The plane between x and y faces this camera.
        let offset = style().plane_offset;
        assert_eq!(
            pick(
                GizmoMode::Translate,
                MIDDLE + Vec2::new(offset, -offset)
            ),
            Some(Handle::Plane(2))
        );

        // The ring around z faces this camera: a circle a handle's
        // length out.
        let around = Vec2::from_angle(1.0);
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + around * style().size),
            Some(Handle::Axis(2))
        );
        // The other two are edge on, and cross at the origin.
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + Vec2::new(0.0, 50.0)),
            Some(Handle::Axis(0))
        );
        assert_eq!(
            pick(
                GizmoMode::Rotate,
                MIDDLE + around * style().outer_ring
            ),
            Some(Handle::View)
        );
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + Vec2::splat(40.0)),
            Some(Handle::Ball)
        );
        assert_eq!(
            pick(GizmoMode::Rotate, MIDDLE + Vec2::splat(100.0)),
            None
        );
    }

    #[test]
    fn a_ring_shows_the_half_that_faces_the_viewer() {
        let look = Vec3::new(0.3, -0.5, -1.0).normalize();
        let arc = Arc::facing(Vec3::Y, look, 0.0);
        assert!((arc.to - arc.from - PI).abs() < 1e-4);
        for point in arc.points(Vec3::ZERO, 1.0, RING_STEPS) {
            assert!(point.dot(look) < 1e-4);
            assert!(point.dot(Vec3::Y).abs() < 1e-4);
        }
        // The middle of it is the point nearest the viewer.
        let nearest = arc.at(0.0);
        assert!(nearest.dot(look) < arc.at(0.1).dot(look));
        assert!(nearest.dot(look) < arc.at(-0.1).dot(look));

        // A little more of it with some overlap.
        let more = Arc::facing(Vec3::Y, look, 0.02);
        assert!(more.to > arc.to && more.to < arc.to + 0.1);

        // All of it face on.
        let whole = Arc::facing(look, look, 0.02);
        assert!((whole.to - whole.from - 2.0 * PI).abs() < 1e-4);
    }

    #[test]
    fn the_hidden_half_of_a_ring_is_out_of_reach() {
        // Tipped toward the viewer, so the ring around y shows as an
        // ellipse with its near half below the origin.
        let tilt = Quat::from_rotation_x(FRAC_PI_6);
        let tilted = Frame {
            axes: Vec3::AXES.map(|axis| tilt * axis),
            ..frame()
        };
        let pick = |angle: f32| {
            let on = Vec3::new(angle.cos(), 0.0, angle.sin());
            let cursor = shown_at(tilt, on, style().size);
            pick(
                GizmoMode::Rotate,
                &tilted,
                cursor,
                &style(),
                project,
            )
        };
        assert_eq!(pick(0.8), Some(Handle::Axis(1)));
        assert_eq!(pick(-0.8), Some(Handle::Ball));
    }

    #[test]
    fn a_handle_fades_as_it_turns_to_the_viewer() {
        let style = style();
        let [gone, full] = style.axis_fade;
        assert_eq!(fade(gone, style.axis_fade), 0.0);
        assert_eq!(fade(full, style.axis_fade), 1.0);
        let half = fade((gone + full) / 2.0, style.axis_fade);
        assert!((half - 0.5).abs() < 1e-4);

        // z points at this camera, and the planes x and y face out
        // of are edge on to it.
        let frame = frame();
        assert_eq!(frame.axis_fade(2, &style), 0.0);
        assert_eq!(frame.axis_fade(0, &style), 1.0);
        assert_eq!(frame.plane_fade(0, &style), 0.0);
        assert_eq!(frame.plane_fade(2, &style), 1.0);
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
    fn a_plane_drag_moves_along_its_two_axes_only() {
        // Seen from a camera off to one side of the plane.
        let ray = |x: f32, y: f32| Pointing {
            cursor: MIDDLE,
            ray: Ray3d::new(
                Vec3::new(x, y, 10.0) + Vec3::new(10.0, 0.0, 0.0),
                Dir3::new(Vec3::new(-1.0, 0.0, -1.0))
                    .expect("it is not zero"),
            ),
        };
        let grab = Grab {
            pointing: ray(0.5, 0.5),
            ..grab(Handle::Plane(2), MIDDLE)
        };

        let delta = grab
            .translation(&ray(1.75, -0.5), None)
            .expect("it moves");
        assert!(delta.abs_diff_eq(Vec3::new(1.25, -1.0, 0.0), 1e-4));

        let snapped = grab
            .translation(&ray(1.75, -0.5), Some(1.0))
            .expect("it moves");
        assert!(snapped.abs_diff_eq(Vec3::new(1.0, -1.0, 0.0), 1e-4));
    }

    #[test]
    fn a_ring_drag_turns_the_way_the_pointer_goes() {
        // From the right of the origin to above it, as drawn.
        let grab =
            grab(Handle::Axis(2), MIDDLE + Vec2::new(100.0, 0.0));
        let now = point_at(MIDDLE + Vec2::new(0.0, -100.0));

        let quarter = turn(&grab, &now, None);
        assert!((quarter * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));

        // The wedge it has swept runs from under the pointer then to
        // under the pointer now.
        let (_, angle) = grab.turn(&now, None).expect("it turns");
        let (arc, radius) =
            grab.sweep(angle, &style()).expect("it has swept");
        assert_eq!(radius, style().size);
        assert!(arc.at(arc.from).abs_diff_eq(Vec3::X, 1e-4));
        assert!(arc.at(arc.to).abs_diff_eq(Vec3::Y, 1e-4));

        // Seen from behind, the same sweep turns the other way.
        let behind = Grab {
            frame: Frame {
                look: Vec3::Z,
                ..frame()
            },
            ..grab
        };
        let back = turn(&behind, &now, None);
        assert!((back * Vec3::X).abs_diff_eq(Vec3::NEG_Y, 1e-4));

        let near = point_at(MIDDLE + Vec2::new(20.0, -100.0));
        let snapped = turn(&grab, &near, Some(FRAC_PI_2));
        assert!((snapped * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));

        // The ring that faces the view turns about the line of
        // sight.
        let facing = Grab {
            handle: Handle::View,
            ..grab
        };
        let about_view = turn(&facing, &now, None);
        assert!((about_view * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));
    }

    #[test]
    fn a_ball_drag_rolls_the_near_side_after_the_pointer() {
        let grab = grab(Handle::Ball, MIDDLE);
        let nearest = Vec3::Z;

        // A handle's length is a radian.
        let right =
            point_at(MIDDLE + Vec2::new(ZOOM * FRAC_PI_2, 0.0));
        let rolled = turn(&grab, &right, None);
        assert!((rolled * nearest).abs_diff_eq(Vec3::X, 1e-4));

        let down =
            point_at(MIDDLE + Vec2::new(0.0, ZOOM * FRAC_PI_2));
        let rolled = turn(&grab, &down, None);
        assert!((rolled * nearest).abs_diff_eq(Vec3::NEG_Y, 1e-4));

        assert!(grab.turn(&point_at(MIDDLE), None).is_none());
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

        let plane = grab(Handle::Plane(2), from);
        let factor = plane.scale(&now, None).expect("it scales");
        assert!(factor.abs_diff_eq(Vec3::new(2.0, 2.0, 1.0), 1e-4));
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
