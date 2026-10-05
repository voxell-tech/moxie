//! The viewport: the scene through a free editor camera, over a
//! ground grid, with the selection's bounds outlined and a gizmo on
//! it.

#![allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Inherent to Bevy ECS: systems take many params and \
              query tuples."
)]

mod camera;
mod gizmo;
mod markers;
mod toolbar;

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraUpdateSystems, RenderTarget};
use bevy::dev_tools::infinite_grid::{
    InfiniteGrid, InfiniteGridPlugin, InfiniteGridSettings,
};
use bevy::ecs::schedule::common_conditions::not;
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
use bevy::ui::UiSystems;
use bevy::ui::widget::ViewportNode;
use bevy_fynix::views::{FrameProps as _, column};
use bevy_fynix::{AnyView, Bevy, Theme};
use bevy_motiongfx::scene::id::EntityUid;
use moxie_ui::theme::EditorTheme;
use moxie_ui::{SelectedEntity, text_field_focused};

pub use self::camera::{
    EditorCamera, ViewportView, ViewportViews, capture_views,
    framing_distance, restore_views,
};
use self::camera::{
    ease_cameras, frame_selected, place_cameras, rest_hidden_cameras,
    view_keys,
};
use self::gizmo::{HandleGizmos, Hot};
use self::markers::{Markers, draw_cameras_and_lights};

/// The render layer of what only a viewport draws.
const EDITOR_LAYER: usize = 2;
/// The render layer of the ground grid, which a viewport can hide.
const GRID_LAYER: usize = 3;

/// Marker component for a camera of the scene.
#[derive(Component, Default, Clone, Copy)]
pub struct SceneCamera;

/// The width of the scene cameras' output over its height.
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct OutputAspect(pub f32);

impl Default for OutputAspect {
    fn default() -> Self {
        Self(16.0 / 9.0)
    }
}

/// Plugin for the viewport [`panel`].
pub fn plugin(app: &mut App) {
    if !app.is_plugin_added::<MeshPickingPlugin>() {
        app.add_plugins(MeshPickingPlugin);
    }
    if !app.is_plugin_added::<InfiniteGridPlugin>() {
        app.add_plugins(InfiniteGridPlugin);
    }
    app.init_gizmo_group::<EditorGizmos>()
        .init_resource::<OutputAspect>()
        .init_resource::<ViewportViews>()
        .init_resource::<SelectedEntity>()
        .add_plugins(gizmo::plugin)
        .add_systems(Startup, (keep_gizmos_to_viewports, spawn_grid))
        .add_systems(
            Update,
            (
                (frame_selected, view_keys)
                    .run_if(not(text_field_focused)),
                ease_cameras,
                place_cameras,
            )
                .chain(),
        )
        // After the layout: a panel built this frame has its size
        // by then, and its camera draws at once.
        .add_systems(
            PostUpdate,
            (reap_orphans, rest_hidden_cameras, follow_screen_scale)
                .chain()
                .after(UiSystems::Layout),
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
        .add_observer(orphan_camera);
}

/// System set for a viewport's overlays, run once the frame's poses
/// and projections are settled.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Overlay;

/// Gizmos only a viewport's camera draws.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct EditorGizmos;

fn keep_gizmos_to_viewports(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<EditorGizmos>();
    config.render_layers = RenderLayers::layer(EDITOR_LAYER);
}

/// Draws what is sized in pixels as large on one screen as on
/// another: each viewport's camera counts in logical pixels, and
/// its lines, whose width is in physical ones, are widened to match.
fn follow_screen_scale(
    nodes: Query<(&ViewportNode, &ComputedNode)>,
    mut targets: Query<&mut RenderTarget, With<EditorCamera>>,
    theme: Res<Theme<EditorTheme>>,
    mut store: ResMut<GizmoConfigStore>,
) {
    for (node, computed) in &nodes {
        if computed.size().min_element() <= 0.0 {
            continue;
        }
        let factor = computed.inverse_scale_factor().recip();
        let target = node
            .camera
            .and_then(|camera| targets.get_mut(camera).ok());
        if let Some(mut target) = target {
            let stale = matches!(
                &*target,
                RenderTarget::Image(image)
                    if image.scale_factor != factor
            );
            if stale && let RenderTarget::Image(image) = &mut *target
            {
                image.scale_factor = factor;
            }
        }

        let outline = GizmoLineConfig::default().width * factor;
        if store.config::<EditorGizmos>().0.line.width != outline {
            store.config_mut::<EditorGizmos>().0.line.width = outline;
        }
        let handle = theme.0.gizmo.line_width * factor;
        if store.config::<HandleGizmos>().0.line.width != handle {
            store.config_mut::<HandleGizmos>().0.line.width = handle;
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
        // The camera of a panel just dropped, as a dock laid out
        // again drops and builds its panels: it keeps its pose, and
        // its image what it last drew.
        let kept = cx
            .world
            .query::<(Entity, &mut EditorCamera)>()
            .iter_mut(cx.world)
            .find(|(_, orbit)| orbit.orphaned)
            .map(|(camera, mut orbit)| {
                orbit.orphaned = false;
                camera
            });
        let ground = cx.theme().color.bg;
        let camera =
            kept.unwrap_or_else(|| spawn_camera(cx.world, ground));
        cx.build(
            column((toolbar::toolbar(camera), surface(camera)))
                .width(percent(100.0))
                .height(percent(100.0))
                .gap(0.0),
        )
    })
}

/// Spawns a viewport's camera, drawing into an image of its own
/// that it clears to `ground`.
fn spawn_camera(world: &mut World, ground: Color) -> Entity {
    // Sized to the node once it is laid out.
    let image = world.resource_mut::<Assets<Image>>().add(
        Image::new_target_texture(
            1,
            1,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ),
    );
    // The view the project saved for a viewport opened this far
    // along, when it saved one.
    let opened = world
        .query::<&EditorCamera>()
        .iter(world)
        .filter(|camera| !camera.orphaned)
        .count();
    let mut orbit = EditorCamera::default();
    let saved = world
        .get_resource::<ViewportViews>()
        .and_then(|views| views.0.get(opened));
    if let Some(view) = saved {
        orbit.restore(view);
    }
    world
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
        .id()
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

/// Leaves a viewport's camera for the next panel built to take up.
fn orphan_camera(
    removed: On<Remove, ViewportNode>,
    nodes: Query<&ViewportNode>,
    mut cameras: Query<&mut EditorCamera>,
) {
    let camera = nodes
        .get(removed.entity)
        .ok()
        .and_then(|node| node.camera)
        .and_then(|camera| cameras.get_mut(camera).ok());
    if let Some(mut camera) = camera {
        camera.orphaned = true;
    }
}

/// Despawns the cameras no panel took up.
fn reap_orphans(
    cameras: Query<(Entity, &EditorCamera)>,
    mut commands: Commands,
) {
    for (camera, orbit) in &cameras {
        if orbit.orphaned {
            commands.entity(camera).try_despawn();
        }
    }
}
