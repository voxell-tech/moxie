//! A viewport's camera: its orbit, the views it shows, and where
//! it is put each frame.

use core::f32::consts::FRAC_PI_2;

use bevy::camera::ScalingMode;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::picking::pointer::PointerLocation;
use bevy::prelude::*;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::Theme;
use moxie_ui::SelectedEntity;
use moxie_ui::theme::{EditorTheme, ViewportControls};

use super::{EDITOR_LAYER, GRID_LAYER, SceneCamera};

/// Short of straight up or down, where the yaw would flip.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

/// A viewport's camera, orbiting `focus`.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    orthographic: bool,
    pub(crate) grid: bool,
    /// Whether it looks through the scene camera, leaving the orbit
    /// as it was for when it stops.
    through: bool,
    /// Whether the panel it drew for is gone, and no other has taken
    /// it up yet.
    pub(crate) orphaned: bool,
}

impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 0.6,
            pitch: -0.4,
            distance: 16.0,
            orthographic: false,
            grid: true,
            through: false,
            orphaned: false,
        }
    }
}

/// The distance a camera with the vertical field of view `fov` sees
/// the whole of a sphere of `radius` from.
pub fn framing_distance(radius: f32, fov: f32) -> f32 {
    radius / (fov / 2.0).sin()
}

impl EditorCamera {
    fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }

    pub(crate) fn transform(&self) -> Transform {
        let rotation = self.rotation();
        Transform::from_translation(
            self.focus + rotation * Vec3::Z * self.distance,
        )
        .with_rotation(rotation)
    }

    fn fov() -> f32 {
        PerspectiveProjection::default().fov
    }

    pub(crate) fn projection(&self) -> Projection {
        if !self.orthographic {
            return Projection::Perspective(default());
        }
        // As tall at the focus as the perspective view is, so the
        // switch keeps what is there the same size.
        let height = 2.0 * self.distance * (Self::fov() / 2.0).tan();
        let far = OrthographicProjection::default_3d().far;
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: height,
            },
            // Nothing is too near to an orthographic view.
            near: -far,
            ..OrthographicProjection::default_3d()
        })
    }

    pub(crate) fn layers(&self) -> RenderLayers {
        let layers = RenderLayers::from_layers(&[0, EDITOR_LAYER]);
        if self.grid {
            layers.with(GRID_LAYER)
        } else {
            layers
        }
    }

    /// Whether it shows what the scene camera does, with nothing of
    /// the editor's drawn over it.
    pub(crate) fn follows_scene_camera(&self) -> bool {
        self.through
    }

    /// Puts the camera where `pose` is, looking the same way, short
    /// of any roll.
    fn snap_to(&mut self, pose: &Transform) {
        let (yaw, pitch, _) = pose.rotation.to_euler(EulerRot::YXZ);
        self.yaw = yaw;
        self.pitch = pitch.clamp(-MAX_PITCH, MAX_PITCH);
        self.focus = pose.translation
            - self.rotation() * Vec3::Z * self.distance;
    }

    /// Stops looking through the scene camera, staying at `pose`,
    /// where that left it.
    pub(crate) fn free(&mut self, pose: &Transform) {
        if self.through {
            self.snap_to(pose);
            self.through = false;
        }
    }

    /// Looks at a sphere of `radius` around `centre`, from far
    /// enough to see the whole of it.
    fn frame(
        &mut self,
        centre: Vec3,
        radius: f32,
        controls: &ViewportControls,
    ) {
        self.focus = centre;
        self.distance = (framing_distance(radius, Self::fov())
            * controls.frame_margin)
            .clamp(controls.min_distance, controls.max_distance);
    }

    /// Shows `view`. `scene` is where the scene camera is, when
    /// there is one.
    fn show(&mut self, view: View, scene: Option<&Transform>) {
        match view {
            View::Perspective => {
                self.orthographic = false;
                self.through = false;
            }
            View::Orthographic => {
                self.orthographic = true;
                self.through = false;
            }
            View::Front | View::Right | View::Top => {
                if let Some((yaw, pitch)) = view.direction() {
                    self.yaw = yaw;
                    self.pitch = pitch;
                }
                self.orthographic = true;
                self.through = false;
            }
            View::Camera => self.through = scene.is_some(),
        }
    }

    /// The view it shows now.
    pub(crate) fn view(&self) -> View {
        if self.through {
            return View::Camera;
        }
        if !self.orthographic {
            return View::Perspective;
        }
        [View::Front, View::Right, View::Top]
            .into_iter()
            .find(|side| {
                side.direction() == Some((self.yaw, self.pitch))
            })
            .unwrap_or(View::Orthographic)
    }

    pub(crate) fn orbit(
        &mut self,
        delta: Vec2,
        controls: &ViewportControls,
    ) {
        self.yaw -= delta.x * controls.orbit_speed;
        self.pitch = (self.pitch - delta.y * controls.orbit_speed)
            .clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves the focus so that what is as far off as it follows a
    /// pointer moved by `delta`, in a viewport `height` tall, both
    /// in logical pixels.
    pub(crate) fn pan(&mut self, delta: Vec2, height: f32) {
        let rotation = self.rotation();
        // How tall the view is at the focus, in either projection.
        let seen = 2.0 * self.distance * (Self::fov() / 2.0).tan();
        let step = seen / height.max(1.0);
        self.focus += rotation * Vec3::X * -delta.x * step
            + rotation * Vec3::Y * delta.y * step;
    }

    /// Moves toward the focus for a positive `amount`, a share of
    /// the distance.
    pub(crate) fn zoom(
        &mut self,
        amount: f32,
        controls: &ViewportControls,
    ) {
        self.distance = (self.distance * (1.0 - amount))
            .clamp(controls.min_distance, controls.max_distance);
    }
}

/// Where a viewport's camera looks from, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    /// Free, with things farther off drawn smaller.
    Perspective,
    /// Free, with things the same size however far off.
    Orthographic,
    Front,
    Right,
    Top,
    /// Through the scene camera.
    Camera,
}

/// The key that moves a viewport to where the scene camera is.
const SNAP_KEY: KeyCode = KeyCode::NumpadDecimal;

impl View {
    pub(crate) const ALL: [Self; 6] = [
        Self::Perspective,
        Self::Orthographic,
        Self::Front,
        Self::Right,
        Self::Top,
        Self::Camera,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Perspective => "Perspective",
            Self::Orthographic => "Orthographic",
            Self::Front => "Front",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Camera => "Camera",
        }
    }

    /// The yaw and pitch a view down an axis looks from.
    fn direction(self) -> Option<(f32, f32)> {
        match self {
            Self::Front => Some((0.0, 0.0)),
            Self::Right => Some((FRAC_PI_2, 0.0)),
            Self::Top => Some((0.0, -FRAC_PI_2)),
            _ => None,
        }
    }

    fn key(self) -> Option<KeyCode> {
        match self {
            Self::Perspective => None,
            Self::Orthographic => Some(KeyCode::Numpad5),
            Self::Front => Some(KeyCode::Numpad1),
            Self::Right => Some(KeyCode::Numpad3),
            Self::Top => Some(KeyCode::Numpad7),
            Self::Camera => Some(KeyCode::Numpad0),
        }
    }
}

/// A 3D camera of the scene, which a viewport can look through.
pub(crate) type Lens =
    (With<Camera3d>, With<SceneCamera>, Without<EditorCamera>);

/// The scene camera a viewport looks through: the selected one, or
/// else the first there is.
fn lens<'a>(
    selected: &SelectedEntity,
    cameras: &'a Query<(&GlobalTransform, &Projection), Lens>,
) -> Option<(&'a GlobalTransform, &'a Projection)> {
    selected
        .0
        .and_then(|selected| cameras.get(selected).ok())
        .or_else(|| cameras.iter().next())
}

/// Shows `view` in the viewport camera `camera`.
pub(crate) fn show(
    In((camera, view)): In<(Entity, View)>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let scene = lens(&selected, &scene)
        .map(|(pose, _)| pose.compute_transform());
    if let Ok(mut orbit) = cameras.get_mut(camera) {
        orbit.show(view, scene.as_ref());
    }
}

/// Shows the [`View`] whose key is pressed, or snaps to the scene
/// camera, in the viewport the pointer is in. A key pressed in the
/// view it leads to leaves it.
pub(crate) fn view_keys(
    keys: Res<ButtonInput<KeyCode>>,
    nodes: Query<(&ViewportNode, &PointerLocation)>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let view = View::ALL.into_iter().find(|view| {
        view.key().is_some_and(|key| keys.just_pressed(key))
    });
    let snap = keys.just_pressed(SNAP_KEY);
    if view.is_none() && !snap {
        return;
    }
    let scene = lens(&selected, &scene)
        .map(|(pose, _)| pose.compute_transform());
    for (node, pointer) in &nodes {
        let camera = node
            .camera
            .filter(|_| pointer.location.is_some())
            .and_then(|camera| cameras.get_mut(camera).ok());
        let Some(mut camera) = camera else {
            continue;
        };
        match view {
            Some(View::Orthographic) if camera.orthographic => {
                camera.show(View::Perspective, None);
            }
            Some(View::Camera) if camera.through => {
                camera.through = false;
            }
            Some(view) => camera.show(view, scene.as_ref()),
            None => {}
        }
        if let Some(scene) = scene.as_ref().filter(|_| snap) {
            camera.snap_to(scene);
            camera.through = false;
        }
    }
}

/// Puts each viewport's camera where its orbit says, or where the
/// scene camera is while it looks through that.
pub(crate) fn place_cameras(
    mut cameras: Query<(
        &mut EditorCamera,
        &mut Transform,
        &mut Projection,
        &mut RenderLayers,
    )>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
) {
    let scene = lens(&selected, &scene);
    for (mut orbit, mut transform, mut projection, mut layers) in
        &mut cameras
    {
        match scene.filter(|_| orbit.follows_scene_camera()) {
            Some((pose, lens)) => {
                transform.set_if_neq(pose.compute_transform());
                *projection = lens.clone();
                layers.set_if_neq(RenderLayers::layer(0));
            }
            None => {
                // The scene camera it looked through is gone.
                if orbit.through {
                    orbit.through = false;
                }
                if orbit.is_changed() {
                    transform.set_if_neq(orbit.transform());
                    layers.set_if_neq(orbit.layers());
                    *projection = orbit.projection();
                }
            }
        }
    }
}

/// Keeps a viewport's camera from rendering while its panel is not
/// on screen.
pub(crate) fn rest_hidden_cameras(
    nodes: Query<(&ViewportNode, &ComputedNode)>,
    mut cameras: Query<&mut Camera, With<EditorCamera>>,
) {
    for (node, computed) in &nodes {
        let Some(mut camera) = node
            .camera
            .and_then(|camera| cameras.get_mut(camera).ok())
        else {
            continue;
        };
        // A hidden tab is laid out with no size.
        let shown = computed.size().min_element() > 0.0;
        if camera.is_active != shown {
            camera.is_active = shown;
        }
    }
}

/// The sphere around the bounds of `root` and everything under it,
/// in the world. `None` when nothing there has bounds.
fn bounding_sphere(
    root: Entity,
    children: &Query<&Children>,
    bounds: &Query<(&Aabb, &GlobalTransform)>,
) -> Option<(Vec3, f32)> {
    let corners = core::iter::once(root)
        .chain(children.iter_descendants(root))
        .filter_map(|entity| bounds.get(entity).ok())
        .flat_map(|(aabb, transform)| {
            let centre = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            (0..8).map(move |corner| {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                transform.transform_point(centre + half * sign)
            })
        });
    let (min, max) = corners.fold(None, |span, corner| {
        let (min, max) = span.unwrap_or((corner, corner));
        Some((min.min(corner), max.max(corner)))
    })?;
    Some(((min + max) / 2.0, (max - min).length() / 2.0))
}

/// Frames the selection in every viewport when F is pressed.
pub(crate) fn frame_selected(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<SelectedEntity>,
    theme: Res<Theme<EditorTheme>>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    transforms: Query<&GlobalTransform>,
    mut cameras: Query<&mut EditorCamera>,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    let controls = &theme.0.viewport;
    let Some((centre, radius)) = selected.0.and_then(|root| {
        bounding_sphere(root, &children, &bounds).or_else(|| {
            let at = transforms.get(root).ok()?.translation();
            Some((at, controls.frame_radius))
        })
    }) else {
        return;
    };
    for mut camera in &mut cameras {
        camera.frame(centre, radius, controls);
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

    fn controls() -> ViewportControls {
        EditorTheme::default().viewport
    }

    #[test]
    fn a_pan_moves_the_focus_and_keeps_the_distance() {
        let mut orbit = EditorCamera::default();
        let before = orbit;
        orbit.pan(Vec2::new(40.0, -25.0), 600.0);

        assert_ne!(orbit.focus, before.focus);
        assert_eq!(orbit.distance, before.distance);
        // Across the view, never toward or away from the camera.
        let moved = orbit.focus - before.focus;
        assert!(moved.dot(before.rotation() * Vec3::Z).abs() < 1e-4);

        // A drag the height of the viewport moves it by all the
        // view shows at the focus.
        let mut orbit = before;
        orbit.pan(Vec2::new(0.0, 600.0), 600.0);
        let seen =
            2.0 * before.distance * (EditorCamera::fov() / 2.0).tan();
        let moved = (orbit.focus - before.focus).length();
        assert!((moved - seen).abs() < 1e-3);
    }

    #[test]
    fn framing_backs_off_until_the_bounds_fit() {
        let mut orbit = EditorCamera::default();
        let centre = Vec3::new(4.0, 1.0, -2.0);
        for radius in [0.1, 3.0, 250.0] {
            orbit.frame(centre, radius, &controls());

            assert_eq!(orbit.focus, centre);
            // The sphere's edge is inside the half angle of the view.
            let half = EditorCamera::fov() / 2.0;
            assert!(radius / orbit.distance < half.sin());
        }
    }

    #[test]
    fn snapping_to_a_pose_takes_its_place_and_direction() {
        let pose = Transform::from_xyz(3.0, 4.0, 5.0)
            .looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y);
        let mut orbit = EditorCamera {
            through: true,
            ..default()
        };
        orbit.free(&pose);

        assert!(!orbit.through);
        let placed = orbit.transform();
        assert!(
            placed.translation.abs_diff_eq(pose.translation, 1e-4)
        );
        assert!(placed.forward().dot(*pose.forward()) > 0.9999);
    }
}
