//! The scene cameras' plumbing: pointing them at the offscreen
//! preview image, one over another, and keeping that image the
//! project's size.

use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;

use crate::thumbnails::ThumbnailCamera;
use crate::ui::TrackViewportCamera;
use crate::viewport::EditorCamera;
use crate::{PreviewImage, ProjectSettings};

/// A camera of the scene: every one but the editor's own.
pub(crate) type SceneCamera = (
    With<Camera>,
    Without<TrackViewportCamera>,
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
/// preview is on screen: the 3D ones first and the 2D ones over
/// them, the first clearing to the project's background.
pub(crate) fn sync_scene_cameras(
    mut commands: Commands,
    preview: Res<PreviewImage>,
    settings: Res<ProjectSettings>,
    mut rendering: ResMut<Rendering>,
    panels: Query<&ComputedNode, With<PreviewPanel>>,
    mut cameras: Query<
        (Entity, &mut Camera, Option<&RenderTarget>, Has<Camera2d>),
        SceneCamera,
    >,
) {
    // A hidden tab is laid out with no size.
    let shown =
        panels.iter().any(|panel| panel.size().min_element() > 0.0);
    rendering.set_if_neq(Rendering(!cameras.is_empty()));
    let mut stack = cameras
        .iter()
        .map(|(entity, .., flat)| (flat, entity))
        .collect::<Vec<_>>();
    stack.sort_unstable();
    for (order, (_, entity)) in stack.into_iter().enumerate() {
        let Ok((entity, mut camera, target, _)) =
            cameras.get_mut(entity)
        else {
            continue;
        };
        if camera.is_active != shown {
            camera.is_active = shown;
        }
        let order = order as isize;
        if camera.order != order {
            camera.order = order;
        }
        let clear = if order == 0 {
            ClearColorConfig::Custom(settings.background)
        } else {
            ClearColorConfig::None
        };
        let cleared = match (&camera.clear_color, &clear) {
            (
                ClearColorConfig::Custom(now),
                ClearColorConfig::Custom(wanted),
            ) => now == wanted,
            (ClearColorConfig::None, ClearColorConfig::None) => true,
            _ => false,
        };
        if !cleared {
            camera.clear_color = clear;
        }
        let targeted = matches!(
            target,
            Some(RenderTarget::Image(target))
                if target.handle == preview.0,
        );
        if !targeted {
            commands.entity(entity).insert(RenderTarget::Image(
                preview.0.clone().into(),
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

/// How large the preview is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewZoom {
    /// As large as fits its panel.
    #[default]
    Fit,
    /// One pixel of the image to one of the screen.
    Actual,
}

impl PreviewZoom {
    pub(crate) const ALL: [Self; 2] = [Self::Fit, Self::Actual];

    /// The logical size an image of `output` pixels is shown at in
    /// an area of `available` logical pixels, on a screen of `scale`
    /// physical pixels to a logical one.
    pub(crate) fn size(
        self,
        output: UVec2,
        available: Vec2,
        scale: f32,
    ) -> Vec2 {
        let output = output.as_vec2();
        match self {
            // The largest of the image's shape the area holds.
            Self::Fit => output * (available / output).min_element(),
            Self::Actual => output / scale,
        }
    }
}

/// The size the preview is shown at in `area`, for the `zoom` asked
/// for. `None` until the area is laid out.
pub(crate) fn preview_size(
    world: &World,
    area: Entity,
    zoom: PreviewZoom,
) -> Option<Vec2> {
    let computed = world.get::<ComputedNode>(area)?;
    let scale = computed.inverse_scale_factor();
    let available = computed.size() * scale;
    if available.min_element() <= 0.0 {
        return None;
    }
    let output = world.resource::<ProjectSettings>().size();
    Some(zoom.size(output, available, scale.recip()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fit_keeps_the_shape_and_actual_size_keeps_the_pixels() {
        let output = UVec2::new(1920, 1080);
        let wide = Vec2::new(800.0, 800.0);
        let tall = Vec2::new(1600.0, 450.0);

        let fit = PreviewZoom::Fit;
        assert_eq!(
            fit.size(output, wide, 2.0),
            Vec2::new(800.0, 450.0)
        );
        assert_eq!(
            fit.size(output, tall, 2.0),
            Vec2::new(800.0, 450.0)
        );
        assert_eq!(
            PreviewZoom::Actual.size(output, wide, 2.0),
            Vec2::new(960.0, 540.0)
        );
    }
}
