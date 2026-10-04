//! The viewport: the scene through a free editor camera, over a
//! ground grid, with the selection's bounds outlined and a gizmo on
//! it.

#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Inherent to Bevy ECS: systems take many params and \
              query tuples."
)]

mod gizmo;
mod toolbar;

use core::f32::consts::{FRAC_PI_2, TAU};

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraUpdateSystems, RenderTarget, ScalingMode};
use bevy::dev_tools::infinite_grid::{
    InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings,
};
use bevy::ecs::schedule::common_conditions::not;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::MouseScrollUnit;
use bevy::math::bounding::Aabb3d;
use bevy::picking::events::{Click, Drag, DragEnd, Pointer, Scroll};
use bevy::picking::hover::HoverMap;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::picking::pointer::{
    PointerButton, PointerId, PointerLocation,
};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::views::{FrameProps as _, column};
use bevy_fynix::{AnyView, Bevy, Theme};
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::theme::{EditorTheme, ViewportControls};
use moxie_ui::{SelectedEntity, text_field_focused};

use self::gizmo::Hot;

/// The render layer of what only a viewport draws.
const EDITOR_LAYER: usize = 2;
/// The render layer of the ground grid, which a viewport can hide.
const GRID_LAYER: usize = 3;
/// The rays drawn off the rim of a directional light's disc.
const LIGHT_RAYS: usize = 8;
/// The sides of the polygon a directional light's disc is drawn as.
const LIGHT_RIM: usize = 32;
/// Short of straight up or down, where the yaw would flip.
const MAX_PITCH: f32 = FRAC_PI_2 - 0.01;

/// Marker component for a camera of the scene, which a viewport
/// draws as a frame and can look through. The app marks them.
#[derive(Component, Default, Clone, Copy)]
pub struct SceneCamera;

/// How many times as wide as tall the scene cameras' output is,
/// which the frame drawn for one is the shape of. The app keeps it.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct OutputAspect(pub f32);

impl Default for OutputAspect {
    fn default() -> Self {
        Self(16.0 / 9.0)
    }
}

/// Adds what a [`panel`] runs on.
pub fn plugin(app: &mut App) {
    if !app.is_plugin_added::<MeshPickingPlugin>() {
        app.add_plugins(MeshPickingPlugin);
    }
    if !app.is_plugin_added::<InfiniteGridPlugin>() {
        app.add_plugins(InfiniteGridPlugin);
    }
    app.init_gizmo_group::<EditorGizmos>()
        .init_resource::<OutputAspect>()
        .init_resource::<SelectedEntity>()
        .add_plugins(gizmo::plugin)
        .add_systems(Startup, (keep_gizmos_to_viewports, spawn_grid))
        .add_systems(
            Update,
            (
                (frame_selected, view_keys)
                    .run_if(not(text_field_focused)),
                place_cameras,
                rest_hidden_cameras,
            )
                .chain(),
        )
        .configure_sets(
            PostUpdate,
            Overlay
                .after(TransformSystems::Propagate)
                .after(CameraUpdateSystems),
        )
        .add_systems(
            PostUpdate,
            (outline_selection, draw_cameras_and_lights)
                .in_set(Overlay),
        )
        .add_observer(drop_camera);
}

/// What draws over a viewport, once this frame's poses and
/// projections are settled: drawn any sooner, it trails a moving
/// camera or subject by a frame.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Overlay;

/// Gizmos only a viewport's camera draws.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct EditorGizmos;

fn keep_gizmos_to_viewports(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<EditorGizmos>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
}

/// A viewport's camera, orbiting `focus`.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    orthographic: bool,
    grid: bool,
    /// Whether it looks through the scene camera, leaving the orbit
    /// as it was for when it stops.
    through: bool,
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

    fn transform(&self) -> Transform {
        let rotation = self.rotation();
        Transform::from_translation(
            self.focus + rotation * Vec3::Z * self.distance,
        )
        .with_rotation(rotation)
    }

    fn fov() -> f32 {
        PerspectiveProjection::default().fov
    }

    fn projection(&self) -> Projection {
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

    fn layers(&self) -> RenderLayers {
        let layers = RenderLayers::from_layers(&[0, EDITOR_LAYER]);
        if self.grid {
            layers.with(GRID_LAYER)
        } else {
            layers
        }
    }

    /// Whether it shows what the scene camera does, with nothing of
    /// the editor's drawn over it.
    fn follows_scene_camera(&self) -> bool {
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
    fn free(&mut self, pose: &Transform) {
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

    /// Does `action`. `scene` is where the scene camera is, when
    /// there is one.
    fn apply(
        &mut self,
        action: ViewAction,
        scene: Option<&Transform>,
    ) {
        match action {
            ViewAction::Perspective => {
                self.orthographic = false;
                self.through = false;
            }
            ViewAction::Orthographic => {
                self.orthographic = true;
                self.through = false;
            }
            ViewAction::Front
            | ViewAction::Right
            | ViewAction::Top => {
                if let Some((yaw, pitch)) = action.direction() {
                    self.yaw = yaw;
                    self.pitch = pitch;
                }
                self.orthographic = true;
                self.through = false;
            }
            ViewAction::Camera => self.through = scene.is_some(),
            ViewAction::SnapToCamera => {
                if let Some(scene) = scene {
                    self.snap_to(scene);
                    self.through = false;
                }
            }
        }
    }

    /// The view it shows now.
    pub(crate) fn view(&self) -> ViewAction {
        if self.through {
            return ViewAction::Camera;
        }
        if !self.orthographic {
            return ViewAction::Perspective;
        }
        [ViewAction::Front, ViewAction::Right, ViewAction::Top]
            .into_iter()
            .find(|side| {
                side.direction() == Some((self.yaw, self.pitch))
            })
            .unwrap_or(ViewAction::Orthographic)
    }

    fn orbit(&mut self, delta: Vec2, controls: &ViewportControls) {
        self.yaw -= delta.x * controls.orbit_speed;
        self.pitch = (self.pitch - delta.y * controls.orbit_speed)
            .clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Moves the focus so that what is as far off as it follows a
    /// pointer moved by `delta`, in a viewport `height` tall, both
    /// in logical pixels.
    fn pan(&mut self, delta: Vec2, height: f32) {
        let rotation = self.rotation();
        // How tall the view is at the focus, in either projection.
        let seen = 2.0 * self.distance * (Self::fov() / 2.0).tan();
        let step = seen / height.max(1.0);
        self.focus += rotation * Vec3::X * -delta.x * step
            + rotation * Vec3::Y * delta.y * step;
    }

    /// Moves toward the focus for a positive `amount`, a share of
    /// the distance.
    fn zoom(&mut self, amount: f32, controls: &ViewportControls) {
        self.distance = (self.distance * (1.0 - amount))
            .clamp(controls.min_distance, controls.max_distance);
    }
}

/// A change of where a viewport's camera looks from, or how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewAction {
    /// Free, with things farther off drawn smaller.
    Perspective,
    /// Free, with things the same size however far off.
    Orthographic,
    Front,
    Right,
    Top,
    /// Through the scene camera.
    Camera,
    /// Moves to where the scene camera is, and stays free.
    SnapToCamera,
}

impl ViewAction {
    /// The views a viewport is in one of, in the order it lists
    /// them.
    pub(crate) const VIEWS: [Self; 6] = [
        Self::Perspective,
        Self::Orthographic,
        Self::Front,
        Self::Right,
        Self::Top,
        Self::Camera,
    ];

    const ALL: [Self; 7] = [
        Self::Perspective,
        Self::Orthographic,
        Self::Front,
        Self::Right,
        Self::Top,
        Self::Camera,
        Self::SnapToCamera,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Perspective => "Perspective",
            Self::Orthographic => "Orthographic",
            Self::Front => "Front",
            Self::Right => "Right",
            Self::Top => "Top",
            Self::Camera => "Camera",
            Self::SnapToCamera => "Snap to camera",
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
            Self::SnapToCamera => Some(KeyCode::NumpadDecimal),
        }
    }
}

/// A 3D camera of the scene, which a viewport can look through.
type Lens =
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

/// Does `action` on the viewport camera `camera`.
pub(crate) fn view(
    In((camera, action)): In<(Entity, ViewAction)>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let scene = lens(&selected, &scene)
        .map(|(pose, _)| pose.compute_transform());
    if let Ok(mut orbit) = cameras.get_mut(camera) {
        orbit.apply(action, scene.as_ref());
    }
}

/// Does the [`ViewAction`] whose key is pressed, on the viewport the
/// pointer is in. A key pressed in the view it leads to leaves it.
fn view_keys(
    keys: Res<ButtonInput<KeyCode>>,
    nodes: Query<(&ViewportNode, &PointerLocation)>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let Some(action) = ViewAction::ALL.into_iter().find(|action| {
        action.key().is_some_and(|key| keys.just_pressed(key))
    }) else {
        return;
    };
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
        match action {
            ViewAction::Orthographic if camera.orthographic => {
                camera.apply(ViewAction::Perspective, None);
            }
            ViewAction::Camera if camera.through => {
                camera.through = false;
            }
            _ => camera.apply(action, scene.as_ref()),
        }
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

/// The viewport panel: its own camera, drawn across the panel under
/// a toolbar.
pub fn panel() -> AnyView<Bevy, EditorTheme> {
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
                orbit.layers(),
                orbit.transform(),
                orbit.projection(),
                orbit,
                Hot::default(),
            ))
            .id();
        cx.build(
            column((toolbar::toolbar(camera), surface(camera)))
                .width(percent(100.0))
                .height(percent(100.0))
                .gap(0.0),
        )
    })
}

/// The node `camera` draws into, and the pointer acts on.
fn surface(camera: Entity) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let node = cx.build(
            column(())
                .width(percent(100.0))
                .grow(1.0)
                .min_height(px(0.0))
                .with(ViewportNode::new(camera)),
        );
        let mut entity = cx.world.entity_mut(node);
        gizmo::watch(&mut entity);
        entity
            .observe(on_drag)
            .observe(on_drag_end)
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

/// On a viewport whose pointer has moved past the theme's click slop
/// since its press, until the press ends.
#[derive(Component)]
struct Swept;

fn on_drag(
    mut drag: On<Pointer<Drag>>,
    nodes: Query<(&ViewportNode, &ComputedNode)>,
    mut cameras: Query<(&mut EditorCamera, &Transform)>,
    keys: Res<ButtonInput<KeyCode>>,
    theme: Res<Theme<EditorTheme>>,
    mut commands: Commands,
) {
    let controls = &theme.0.viewport;
    let node = drag.event_target();
    let Ok((viewport, computed)) = nodes.get(node) else {
        return;
    };
    let height = computed.size().y * computed.inverse_scale_factor();
    let Some((mut camera, pose)) = viewport
        .camera
        .and_then(|camera| cameras.get_mut(camera).ok())
    else {
        return;
    };
    drag.propagate(false);
    if drag.distance.length() > controls.click_slop {
        commands.entity(node).insert(Swept);
    }
    let Some(gesture) =
        Gesture::of(drag.button, alt(&keys), shift(&keys))
    else {
        return;
    };
    camera.free(pose);
    match gesture {
        Gesture::Orbit => camera.orbit(drag.delta, controls),
        Gesture::Pan => camera.pan(drag.delta, height),
    }
}

fn on_drag_end(end: On<Pointer<DragEnd>>, mut commands: Commands) {
    commands.entity(end.event_target()).remove::<Swept>();
}

fn on_scroll(
    mut scroll: On<Pointer<Scroll>>,
    nodes: Query<&ViewportNode>,
    mut cameras: Query<(&mut EditorCamera, &Transform)>,
    theme: Res<Theme<EditorTheme>>,
) {
    let Some((mut camera, pose)) = nodes
        .get(scroll.event_target())
        .ok()
        .and_then(|node| node.camera)
        .and_then(|camera| cameras.get_mut(camera).ok())
    else {
        return;
    };
    scroll.propagate(false);
    let controls = &theme.0.viewport;
    let per_unit = match scroll.unit {
        MouseScrollUnit::Line => controls.zoom_per_line,
        MouseScrollUnit::Pixel => controls.zoom_per_pixel,
    };
    camera.free(pose);
    camera.zoom(scroll.y * per_unit, controls);
}

/// The subject drawn nearest under `pointer`, the one a viewport
/// forwards into its camera's image.
fn subject_under(
    pointer: &PointerId,
    hovered: &HoverMap,
    subjects: &Query<(), With<EntityUid>>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    let nearest = hovered.get(pointer).and_then(|hits| {
        hits.iter()
            .min_by(|(_, a), (_, b)| a.depth.total_cmp(&b.depth))
            .map(|(&entity, _)| entity)
    })?;
    core::iter::once(nearest)
        .chain(parents.iter_ancestors(nearest))
        .find(|&entity| subjects.contains(entity))
}

/// Selects the subject under a click, or nothing for a click on
/// empty space.
fn on_click(
    click: On<Pointer<Click>>,
    nodes: Query<(&ViewportNode, &PointerId)>,
    hot: Query<&Hot>,
    hovered: Res<HoverMap>,
    subjects: Query<(), With<EntityUid>>,
    parents: Query<&ChildOf>,
    swept: Query<(), With<Swept>>,
    locations: Query<&PointerLocation>,
    cameras: Query<(&Camera, &GlobalTransform, &EditorCamera)>,
    markers: Markers,
    mut selected: ResMut<SelectedEntity>,
) {
    // The release of a drag is a click too, fired before the drag
    // ends.
    if click.button != PointerButton::Primary
        || swept.contains(click.event_target())
    {
        return;
    }
    let Ok((node, pointer)) = nodes.get(click.event_target()) else {
        return;
    };
    // A click on a gizmo handle is the gizmo's.
    let on_handle = node
        .camera
        .and_then(|camera| hot.get(camera).ok())
        .is_some_and(|hot| hot.0.is_some());
    if on_handle {
        return;
    }
    // A camera or a light is lines, thin enough that a click near
    // them means them and not what is behind.
    let marker = || {
        let cursor = locations
            .get(click.event_target())
            .ok()?
            .location
            .as_ref()?
            .position;
        let (lens, view, orbit) = cameras.get(node.camera?).ok()?;
        if orbit.follows_scene_camera() {
            return None;
        }
        markers.under(cursor, |point| {
            lens.world_to_viewport(view, point).ok()
        })
    };
    let subject = marker().or_else(|| {
        subject_under(pointer, &hovered, &subjects, &parents)
    });
    selected.set_if_neq(SelectedEntity(subject));
}

/// Puts each viewport's camera where its orbit says, or where the
/// scene camera is while it looks through that.
fn place_cameras(
    mut cameras: Query<(
        Ref<EditorCamera>,
        &mut Transform,
        &mut Projection,
        &mut RenderLayers,
    )>,
    selected: Res<SelectedEntity>,
    scene: Query<(&GlobalTransform, &Projection), Lens>,
) {
    let scene = lens(&selected, &scene);
    for (orbit, mut transform, mut projection, mut layers) in
        &mut cameras
    {
        match scene.filter(|_| orbit.follows_scene_camera()) {
            Some((pose, lens)) => {
                transform.set_if_neq(pose.compute_transform());
                *projection = lens.clone();
                layers.set_if_neq(RenderLayers::layer(0));
            }
            // Its own pose again once there is no scene camera to
            // follow.
            None if orbit.is_changed() || orbit.through => {
                transform.set_if_neq(orbit.transform());
                layers.set_if_neq(orbit.layers());
                if orbit.is_changed() {
                    *projection = orbit.projection();
                }
            }
            None => {}
        }
    }
}

/// Keeps a viewport's camera from rendering while its panel is not
/// on screen.
fn rest_hidden_cameras(
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
fn frame_selected(
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

/// The ground grid, on a layer only a viewport's camera sees.
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
        RenderLayers::layer(GRID_LAYER),
    ));
}

/// Outlines the bounds of the selection and everything under it,
/// and of the subject a click would select.
fn outline_selection(
    mut gizmos: Gizmos<EditorGizmos>,
    theme: Res<Theme<EditorTheme>>,
    selected: Res<SelectedEntity>,
    nodes: Query<(&ViewportNode, &PointerId)>,
    hot: Query<&Hot>,
    hovered: Res<HoverMap>,
    subjects: Query<(), With<EntityUid>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
) {
    let mut outline = |root: Entity, color: Color| {
        for entity in core::iter::once(root)
            .chain(children.iter_descendants(root))
        {
            if let Ok((aabb, transform)) = bounds.get(entity) {
                gizmos.aabb_3d(
                    Aabb3d::new(aabb.center, aabb.half_extents),
                    *transform,
                    color,
                );
            }
        }
    };
    if let Some(root) = selected.0 {
        outline(root, theme.0.color.accent);
    }
    let pointed = nodes
        .iter()
        .filter(|(node, _)| {
            // A gizmo handle is in front of whatever is behind it.
            !node
                .camera
                .and_then(|camera| hot.get(camera).ok())
                .is_some_and(|hot| hot.0.is_some())
        })
        .find_map(|(_, pointer)| {
            subject_under(pointer, &hovered, &subjects, &parents)
        })
        .filter(|&subject| selected.0 != Some(subject));
    if let Some(subject) = pointed {
        outline(subject, theme.0.gizmo.hover);
    }
}

/// Draws what each 3D scene camera sees as a frame it looks out
/// through, and where each directional light shines as rays off a
/// disc. The selected one is in the accent colour.
fn draw_cameras_and_lights(
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
struct Markers<'w, 's> {
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
    fn under(
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

    fn controls() -> ViewportControls {
        EditorTheme::default().viewport
    }

    #[test]
    fn an_orbit_stops_short_of_straight_up_and_down() {
        let mut orbit = EditorCamera::default();
        orbit.orbit(Vec2::new(0.0, 1e6), &controls());
        assert_eq!(orbit.pitch, -MAX_PITCH);
        orbit.orbit(Vec2::new(0.0, -1e6), &controls());
        assert_eq!(orbit.pitch, MAX_PITCH);
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
    fn a_zoom_stays_within_its_bounds() {
        let mut orbit = EditorCamera::default();
        let controls = controls();
        for _ in 0..1000 {
            orbit.zoom(0.9, &controls);
        }
        assert_eq!(orbit.distance, controls.min_distance);
        for _ in 0..1000 {
            orbit.zoom(-10.0, &controls);
        }
        assert_eq!(orbit.distance, controls.max_distance);
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

    #[test]
    fn looking_through_needs_a_scene_camera() {
        let mut orbit = EditorCamera::default();
        orbit.apply(ViewAction::Camera, None);
        assert_eq!(orbit.view(), ViewAction::Perspective);

        let scene = Transform::from_xyz(0.0, 2.0, 14.0);
        orbit.apply(ViewAction::Camera, Some(&scene));
        assert_eq!(orbit.view(), ViewAction::Camera);
        // The orbit is kept for when it stops.
        assert_eq!(
            orbit.transform(),
            EditorCamera::default().transform()
        );
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
