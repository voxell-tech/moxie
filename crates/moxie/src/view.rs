//! The scene cameras' plumbing: pointing them at the offscreen
//! preview image, and keeping that image the project's size.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use moxie_viewport::{EditorCamera, OutputAspect, SceneCamera};

use crate::thumbnails::ThumbnailCamera;
use crate::ui::UiCamera;
use crate::{PreviewImage, ProjectSettings};

/// A camera of the project: every one but the editor's own.
type ProjectCamera = (
    With<Camera>,
    Without<UiCamera>,
    Without<ThumbnailCamera>,
    Without<EditorCamera>,
);

/// On a preview panel's own node.
#[derive(Component)]
pub(crate) struct PreviewPanel;

/// Whether the project has a camera to render through.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Rendering(pub(crate) bool);

/// Renders every scene camera into the [`PreviewImage`] while a
/// preview is on screen. Which draws over which, and what each
/// clears to, is each camera's own.
pub(crate) fn sync_scene_cameras(
    mut commands: Commands,
    preview: Res<PreviewImage>,
    settings: Res<ProjectSettings>,
    mut rendering: ResMut<Rendering>,
    mut aspect: ResMut<OutputAspect>,
    panels: Query<&ComputedNode, With<PreviewPanel>>,
    mut cameras: Query<
        (Entity, &mut Camera, Option<&RenderTarget>),
        ProjectCamera,
    >,
) {
    // A hidden tab is laid out with no size.
    let shown =
        panels.iter().any(|panel| panel.size().min_element() > 0.0);
    rendering.set_if_neq(Rendering(!cameras.is_empty()));
    let output = settings.size().as_vec2();
    aspect.set_if_neq(OutputAspect(output.x / output.y));
    for (entity, mut camera, target) in &mut cameras {
        if camera.is_active != shown {
            camera.is_active = shown;
        }
        let targeted = matches!(
            target,
            Some(RenderTarget::Image(target))
                if target.handle == preview.0,
        );
        if !targeted {
            // Marked as one of the scene's, which a viewport draws
            // and looks through.
            commands.entity(entity).insert((
                RenderTarget::Image(preview.0.clone().into()),
                SceneCamera,
            ));
        }
    }
}

/// Keeps the [`PreviewImage`] as large as the project renders.
pub(crate) fn resize_preview(
    preview: Res<PreviewImage>,
    settings: Res<ProjectSettings>,
    mut images: ResMut<Assets<Image>>,
) {
    let size = settings.size();
    let stale = images
        .get(&preview.0)
        .is_some_and(|image| image.size() != size);
    if !stale {
        return;
    }
    if let Some(mut image) = images.get_mut(&preview.0) {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
    }
}
