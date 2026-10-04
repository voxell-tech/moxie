mod action;
mod assets;
pub(crate) mod hierarchy;
mod inspector;
mod preview;
mod settings;
pub(crate) mod timeline;
mod top_bar;

use bevy::camera::Hdr;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::schedule::common_conditions::not;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::text::EditableText;
use bevy::ui::{IsDefaultUiCamera, UiTargetCamera};
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockRegistry, DockTree,
    DockWindowKind, Edge, dock,
};
use bevy_fynix::views::{FrameProps as _, column};
use bevy_fynix::{AnyView, Bevy, mount};
use bevy_motiongfx::motiongfx::field_path::field;
use moxie_ui::MoxieUiPlugin;
use moxie_ui::field_icon::FieldIconAppExt as _;
use moxie_ui::inspector::InspectAppExt as _;
use moxie_ui::theme::{EditorTheme, Hue};

use crate::subject::Target;
use crate::{
    EditorSettings, EditorState, PreviewImage, ProjectBookmarks,
    ProjectPath, ProjectSettings, SelectedAction, SelectedEntity,
    playback, scene, view,
};

/// Wires the editor UI tree and the per-frame
/// timeline/playback/preview systems.
pub(crate) struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((MoxieUiPlugin, timeline::TimelinePlugin))
            .insert_resource(moxie_ui::inspector::FieldAnimatable(
                Some(|world, field| {
                    Target::of(world, field).is_some_and(|target| {
                        target.is_animatable(world)
                    })
                }),
            ))
            .insert_resource(moxie_ui::inspector::FieldHasAction(
                Some(|world, field| {
                    Target::of(world, field).is_some_and(|target| {
                        target.has_action(world)
                    })
                }),
            ))
            .register_field_icon(
                field!(Transform.translation),
                crate::icons::TRANSLATE,
            )
            .register_field_icon(
                field!(Transform.rotation),
                crate::icons::ROTATE,
            )
            .register_field_icon(
                field!(Transform.scale),
                crate::icons::SCALE,
            )
            .register_root_hue::<Transform>(Hue::Blue)
            .register_root_hue::<Visibility>(Hue::Purple)
            .register_root_hue::<StandardMaterial>(Hue::Orange)
            .register_root_hue::<Projection>(Hue::Green)
            .register_root_hue::<PointLight>(Hue::Yellow)
            .register_root_hue::<DirectionalLight>(Hue::Yellow)
            .register_root_hue::<SpotLight>(Hue::Yellow)
            .register_root_hue::<RectLight>(Hue::Yellow)
            .init_resource::<EditorState>()
            .init_resource::<preview::PreviewView>()
            .init_resource::<view::Rendering>()
            .init_resource::<SelectedAction>()
            .init_resource::<SelectedEntity>()
            .init_resource::<ProjectBookmarks>()
            .init_resource::<ProjectPath>()
            .init_resource::<assets::AssetFoldState>()
            .init_resource::<hierarchy::Dragging>()
            .init_resource::<scene::EditorScene>()
            .add_systems(
                Startup,
                (setup_editor_ui, mount_editor_ui).chain(),
            )
            .add_systems(
                Update,
                (
                    scene::recompile_dirty_scene
                        .run_if(scene::scene_dirty),
                    playback::track_first_timeline,
                    playback::play_pause_hotkey
                        .run_if(not(text_field_focused)),
                    playback::stop_at_track_end,
                    playback::track_playing,
                    view::resize_preview,
                    view::sync_scene_cameras,
                )
                    .chain(),
            )
            .add_observer(playback::on_toggle_playback);

        app.with_inspect_group("Cameras")
            .register_inspectable::<Projection>();
    }
}

/// True while a text field holds focus.
pub(crate) fn text_field_focused(
    focus: Res<InputFocus>,
    q_editable: Query<(), With<EditableText>>,
) -> bool {
    focus
        .get()
        .is_some_and(|entity| q_editable.contains(entity))
}

/// Marker component for the UI camera, which owns the window.
#[derive(Component, Default, Clone)]
pub(crate) struct TrackViewportCamera;

fn setup_editor_ui(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut registry: ResMut<DockRegistry<EditorTheme>>,
    mut tree: ResMut<DockTree>,
    settings: Res<EditorSettings>,
    project: Res<ProjectSettings>,
    assets: Res<AssetServer>,
) {
    let size = project.size();
    let preview = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    commands.insert_resource(PreviewImage(preview.clone()));

    // Own render layer so this camera doesn't also pick up scene
    // meshes (e.g. bevy_vello's composite quad, layer 0)
    // full-window. `IsDefaultUiCamera` catches dock UI spawned
    // without a target (drag ghosts, drop overlays).
    let ui_camera = commands
        .spawn_scene(bsn! [
            Camera2d
            Camera {
                order: 10,
                // The const: this runs at startup, before the kernel
                // holds one.
                clear_color: { moxie_ui::theme::BG },
            }
            TrackViewportCamera
        ])
        .insert((RenderLayers::layer(1), IsDefaultUiCamera))
        .id();

    if settings.hdr {
        commands.entity(ui_camera).insert(Hdr);
    }

    register_windows(&mut registry, &assets);

    //
    // The dock layout.
    //
    let viewport = tree.set_root_leaf(
        DockLeaf::new("viewport", DockAreaStyle::TabBar)
            .with_windows(vec!["viewport".into()]),
    );

    tree.split(viewport, Edge::Bottom, "timeline".into());
    let vsplit = tree.root.expect("root split exists");
    tree.set_fraction(vsplit, 0.7);
    let timeline = tree
        .find_leaf_with_window("timeline")
        .expect("just split in a timeline leaf");
    if let Some(DockNode::Leaf(leaf)) = tree.get_mut(timeline) {
        leaf.area_id = "timeline".into();
    }

    tree.split(timeline, Edge::Right, "action".into());
    if let Some(hsplit) = tree.parent_of(timeline) {
        tree.set_fraction(hsplit, 0.8);
    }

    tree.split(viewport, Edge::Right, "inspector".into());
    if let Some(hsplit) = tree.parent_of(viewport) {
        tree.set_fraction(hsplit, 0.8);
    }

    if let Some((sidebar, hierarchy_tab)) =
        tree.split(viewport, Edge::Left, "hierarchy".into())
    {
        // `add_tab` activates what it just added; Hierarchy stays the
        // one shown on a fresh layout.
        tree.add_tab(sidebar, "assets");
        tree.set_active(sidebar, hierarchy_tab);
    }
    if let Some(hsplit) = tree.parent_of(viewport) {
        tree.set_fraction(hsplit, 0.2);
    }

    tree.split(viewport, Edge::Right, "preview".into());
}

/// Mounts the top bar over the dock, on the UI camera. Runs after
/// [`setup_editor_ui`] has applied its commands, so the preview image
/// and the camera exist.
fn mount_editor_ui(world: &mut World) {
    let camera = world
        .query_filtered::<Entity, With<TrackViewportCamera>>()
        .single(world)
        .expect("the UI camera was just spawned");
    let root = mount::<EditorTheme>(
        world,
        column((
            top_bar::top_bar(),
            column((dock::<EditorTheme>(),))
                .width(percent(100.0))
                .grow(1.0)
                .min_height(px(0.0)),
        ))
        .gap(0.0)
        .width(percent(100.0))
        .height(percent(100.0)),
    );
    world.entity_mut(root).insert(UiTargetCamera(camera));
}

/// Register the editor's dockable windows.
fn register_windows(
    registry: &mut DockRegistry<EditorTheme>,
    assets: &AssetServer,
) {
    let kind =
        |name: &'static str,
         icon: &'static str,
         build: fn() -> AnyView<Bevy, EditorTheme>| {
            DockWindowKind::new(name, build).icon(assets.load(icon))
        };

    registry
        .register(
            "viewport",
            kind(
                "Viewport",
                crate::icons::VIEWPORT,
                crate::viewport::panel,
            ),
        )
        .register(
            "preview",
            kind("Preview", crate::icons::PREVIEW, preview::panel),
        )
        .register(
            "timeline",
            kind("Timeline", crate::icons::TIMELINE, timeline::panel),
        )
        .register(
            "hierarchy",
            kind(
                "Hierarchy",
                crate::icons::HIERARCHY,
                hierarchy::panel,
            ),
        )
        .register(
            "action",
            kind("Action", crate::icons::ACTION, action::panel),
        )
        .register(
            "inspector",
            kind(
                "Inspector",
                crate::icons::INSPECTOR,
                inspector::panel,
            ),
        )
        .register(
            "settings",
            kind("Settings", crate::icons::SETTINGS, settings::panel),
        )
        .register(
            "assets",
            kind("Assets", crate::icons::ASSETS, assets::panel),
        );
}
