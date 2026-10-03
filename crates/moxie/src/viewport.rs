//! The viewport: the scene through a free editor camera, over a
//! ground grid and with the selection's bounds outlined.

use core::f32::consts::FRAC_PI_2;

use bevy::camera::RenderTarget;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::dev_tools::infinite_grid::{
    InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings,
};
use bevy::ecs::schedule::common_conditions::not;
use bevy::input::mouse::MouseScrollUnit;
use bevy::math::bounding::Aabb3d;
use bevy::picking::events::{Click, Drag, Pointer, Scroll};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::views::{FrameProps as _, column};
use bevy_fynix::{AnyView, Bevy, Theme};
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::theme::EditorTheme;

use crate::SelectedEntity;
use crate::ui::text_field_focused;

/// The render layer of what only a viewport draws.
const EDITOR_LAYER: usize = 2;

/// Radians a pixel of drag orbits by.
const ORBIT_SPEED: f32 = 0.005;
/// The share of the distance to the focus a pixel of drag pans by.
const PAN_SPEED: f32 = 0.0015;
/// The share of the distance a scrolled line, or pixel, zooms by.
const ZOOM_PER_LINE: f32 = 0.1;
const ZOOM_PER_PIXEL: f32 = 0.002;
const MIN_DISTANCE: f32 = 0.05;
const MAX_DISTANCE: f32 = 10_000.0;
/// Short of straight up or down, where the yaw would flip.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

pub(crate) fn plugin(app: &mut App) {
    if !app.is_plugin_added::<MeshPickingPlugin>() {
        app.add_plugins(MeshPickingPlugin);
    }
    if !app.is_plugin_added::<InfiniteGridPlugin>() {
        app.add_plugins(InfiniteGridPlugin);
    }
    app.init_gizmo_group::<EditorGizmos>()
        .add_systems(Startup, (keep_gizmos_to_viewports, spawn_grid))
        .add_systems(
            Update,
            (
                frame_selected.run_if(not(text_field_focused)),
                place_cameras,
                outline_selection,
            )
                .chain(),
        )
        .add_observer(drop_camera);
}

/// Gizmos only a viewport's camera draws.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct EditorGizmos;

fn keep_gizmos_to_viewports(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<EditorGizmos>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
}

/// A viewport's camera, orbiting `focus`.
#[derive(Component, Clone, Copy, PartialEq)]
pub(crate) struct EditorCamera {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 0.6,
            pitch: -0.4,
            distance: 16.0,
        }
    }
}

impl EditorCamera {
    fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    fn transform(&self) -> Transform {
        let rotation = self.rotation();
        Transform::from_translation(
            self.focus + rotation * Vec3::Z * self.distance,
        )
        .with_rotation(rotation)
    }

    fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SPEED;
        self.pitch = (self.pitch - delta.y * ORBIT_SPEED)
            .clamp(-MAX_PITCH, MAX_PITCH);
    }

    fn pan(&mut self, delta: Vec2) {
        let rotation = self.rotation();
        let step = self.distance * PAN_SPEED;
        self.focus += rotation * Vec3::X * -delta.x * step
            + rotation * Vec3::Y * delta.y * step;
    }

    /// Moves toward the focus for a positive `amount`, a share of
    /// the distance.
    fn zoom(&mut self, amount: f32) {
        self.distance = (self.distance * (1.0 - amount))
            .clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

/// What a drag in a viewport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gesture {
    Orbit,
    Pan,
}

impl Gesture {
    /// Right drag orbits and middle drag pans. Alt makes a left drag
    /// orbit, for a pointer with one button, and shift turns either
    /// orbit into a pan.
    fn of(
        button: PointerButton,
        alt: bool,
        shift: bool,
    ) -> Option<Self> {
        match (button, alt, shift) {
            (PointerButton::Middle, ..) => Some(Self::Pan),
            (PointerButton::Secondary, _, true)
            | (PointerButton::Primary, true, true) => Some(Self::Pan),
            (PointerButton::Secondary, _, false)
            | (PointerButton::Primary, true, false) => {
                Some(Self::Orbit)
            }
            (PointerButton::Primary, false, _) => None,
        }
    }
}

/// The viewport panel: its own camera, drawn across the panel.
pub(crate) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let ground = cx.theme().color.bg;
        // Sized to the node once it is laid out.
        let image = cx.world.resource_mut::<Assets<Image>>().add(
            Image::new_target_texture(
                1,
                1,
                TextureFormat::Rgba8Unorm,
                Some(TextureFormat::Rgba8UnormSrgb),
            ),
        );
        let orbit = EditorCamera::default();
        let camera = cx
            .world
            .spawn((
                Camera3d::default(),
                Camera {
                    clear_color: ground.into(),
                    ..default()
                },
                RenderTarget::Image(image.into()),
                RenderLayers::from_layers(&[0, EDITOR_LAYER]),
                orbit.transform(),
                orbit,
            ))
            .id();
        let node = cx.build(
            column(())
                .width(percent(100.0))
                .height(percent(100.0))
                .with(ViewportNode::new(camera)),
        );
        cx.world
            .entity_mut(node)
            .observe(on_drag)
            .observe(on_scroll)
            .observe(on_click);
        node
    })
}

fn alt(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
}

fn shift(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
}

fn on_drag(
    mut drag: On<Pointer<Drag>>,
    nodes: Query<&ViewportNode>,
    mut cameras: Query<&mut EditorCamera>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let Some(mut camera) = nodes
        .get(drag.event_target())
        .ok()
        .and_then(|node| node.camera)
        .and_then(|camera| cameras.get_mut(camera).ok())
    else {
        return;
    };
    drag.propagate(false);
    match Gesture::of(drag.button, alt(&keys), shift(&keys)) {
        Some(Gesture::Orbit) => camera.orbit(drag.delta),
        Some(Gesture::Pan) => camera.pan(drag.delta),
        None => {}
    }
}

fn on_scroll(
    mut scroll: On<Pointer<Scroll>>,
    nodes: Query<&ViewportNode>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let Some(mut camera) = nodes
        .get(scroll.event_target())
        .ok()
        .and_then(|node| node.camera)
        .and_then(|camera| cameras.get_mut(camera).ok())
    else {
        return;
    };
    scroll.propagate(false);
    let per_unit = match scroll.unit {
        MouseScrollUnit::Line => ZOOM_PER_LINE,
        MouseScrollUnit::Pixel => ZOOM_PER_PIXEL,
    };
    camera.zoom(scroll.y * per_unit);
}

/// Selects the subject under a click, or nothing for a click on
/// empty space.
fn on_click(
    click: On<Pointer<Click>>,
    pointers: Query<&PointerId, With<ViewportNode>>,
    hovered: Res<HoverMap>,
    subjects: Query<(), With<EntityUid>>,
    parents: Query<&ChildOf>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selected: ResMut<SelectedEntity>,
) {
    // An alt click is an orbit let go.
    if click.button != PointerButton::Primary || alt(&keys) {
        return;
    }
    // The pointer the node forwards into its camera's image, which
    // is the one that hovers the scene.
    let Ok(pointer) = pointers.get(click.event_target()) else {
        return;
    };
    let nearest = hovered.get(pointer).and_then(|hits| {
        hits.iter()
            .min_by(|(_, a), (_, b)| a.depth.total_cmp(&b.depth))
            .map(|(&entity, _)| entity)
    });
    let subject = nearest.and_then(|hit| {
        core::iter::once(hit)
            .chain(parents.iter_ancestors(hit))
            .find(|&entity| subjects.contains(entity))
    });
    selected.set_if_neq(SelectedEntity(subject));
}

fn place_cameras(
    mut cameras: Query<
        (&EditorCamera, &mut Transform),
        Changed<EditorCamera>,
    >,
) {
    for (orbit, mut transform) in &mut cameras {
        *transform = orbit.transform();
    }
}

/// Centres every viewport on the selection when F is pressed.
fn frame_selected(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<SelectedEntity>,
    transforms: Query<&GlobalTransform>,
    mut cameras: Query<&mut EditorCamera>,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let Some(focus) = selected
        .0
        .and_then(|entity| transforms.get(entity).ok())
        .map(GlobalTransform::translation)
    else {
        return;
    };
    for mut camera in &mut cameras {
        camera.focus = focus;
    }
}

/// The ground grid, on the layer only a viewport's camera sees.
fn spawn_grid(
    mut commands: Commands,
    theme: Res<Theme<EditorTheme>>,
) {
    let palette = &theme.0.palette;
    commands.spawn((
        InfiniteGrid,
        InfiniteGridSettings {
            x_axis_color: palette.red,
            z_axis_color: palette.blue,
            minor_line_color: palette.base[2],
            major_line_color: palette.base[3],
            ..default()
        },
        RenderLayers::layer(EDITOR_LAYER),
    ));
}

/// Outlines the bounds of the selection and everything under it.
fn outline_selection(
    mut gizmos: Gizmos<EditorGizmos>,
    theme: Res<Theme<EditorTheme>>,
    selected: Res<SelectedEntity>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
) {
    let color = theme.0.color;
    let Some(root) = selected.0 else {
        return;
    };
    for entity in
        core::iter::once(root).chain(children.iter_descendants(root))
    {
        if let Ok((aabb, transform)) = bounds.get(entity) {
            gizmos.aabb_3d(
                Aabb3d::new(aabb.center, aabb.half_extents),
                *transform,
                color.accent,
            );
        }
    }
}

/// Despawns a viewport's camera along with its panel.
fn drop_camera(
    removed: On<Remove, ViewportNode>,
    nodes: Query<&ViewportNode>,
    cameras: Query<(), With<EditorCamera>>,
    mut commands: Commands,
) {
    let camera = nodes
        .get(removed.entity)
        .ok()
        .and_then(|node| node.camera)
        .filter(|&camera| cameras.contains(camera));
    if let Some(camera) = camera {
        commands.entity(camera).try_despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_looks_at_its_focus_from_its_distance() {
        let orbit = EditorCamera {
            focus: Vec3::new(1.0, 2.0, 3.0),
            ..default()
        };
        let transform = orbit.transform();

        let to_focus = orbit.focus - transform.translation;
        assert!((to_focus.length() - orbit.distance).abs() < 1e-4);
        assert!(
            transform.forward().dot(to_focus.normalize()) > 0.9999
        );
    }

    #[test]
    fn an_orbit_stops_short_of_straight_up_and_down() {
        let mut orbit = EditorCamera::default();
        orbit.orbit(Vec2::new(0.0, 1e6));
        assert_eq!(orbit.pitch, -MAX_PITCH);
        orbit.orbit(Vec2::new(0.0, -1e6));
        assert_eq!(orbit.pitch, MAX_PITCH);
    }

    #[test]
    fn a_pan_moves_the_focus_and_keeps_the_distance() {
        let mut orbit = EditorCamera::default();
        let before = orbit;
        orbit.pan(Vec2::new(40.0, -25.0));

        assert_ne!(orbit.focus, before.focus);
        assert_eq!(orbit.distance, before.distance);
        // Across the view, never toward or away from the camera.
        let moved = orbit.focus - before.focus;
        assert!(moved.dot(before.rotation() * Vec3::Z).abs() < 1e-4);
    }

    #[test]
    fn a_zoom_stays_within_its_bounds() {
        let mut orbit = EditorCamera::default();
        for _ in 0..1000 {
            orbit.zoom(0.9);
        }
        assert_eq!(orbit.distance, MIN_DISTANCE);
        for _ in 0..1000 {
            orbit.zoom(-10.0);
        }
        assert_eq!(orbit.distance, MAX_DISTANCE);
    }

    #[test]
    fn modifiers_pick_the_gesture() {
        use PointerButton::{Middle, Primary, Secondary};
        assert_eq!(Gesture::of(Primary, false, false), None);
        assert_eq!(
            Gesture::of(Secondary, false, false),
            Some(Gesture::Orbit)
        );
        assert_eq!(
            Gesture::of(Primary, true, false),
            Some(Gesture::Orbit)
        );
        assert_eq!(
            Gesture::of(Secondary, false, true),
            Some(Gesture::Pan)
        );
        assert_eq!(
            Gesture::of(Primary, true, true),
            Some(Gesture::Pan)
        );
        assert_eq!(
            Gesture::of(Middle, false, false),
            Some(Gesture::Pan)
        );
    }
}
