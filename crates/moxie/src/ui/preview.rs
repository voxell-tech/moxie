//! The preview panel: the project as its cameras see it, at the
//! project's own resolution.

use bevy::picking::events::{Drag, Pointer};
use bevy::prelude::*;
use bevy::ui::widget::ImageNode;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, column, frame, label,
    row, segmented,
};
use bevy_fynix::{
    AnyView, Bevy, View, ViewExt as _, keyed, resource,
};
use moxie_ui::gaps::{anchored, changing_under};
use moxie_ui::theme::EditorTheme;

use super::hierarchy;
use crate::view::{self, PreviewPanel, PreviewZoom, Rendering};
use crate::{PreviewImage, ProjectSettings};

/// How the preview shows the render.
#[derive(Resource, Clone, Copy, Default, PartialEq)]
pub(crate) struct PreviewView {
    zoom: PreviewZoom,
    /// How far the image is dragged off the centre, which only one
    /// larger than its panel can be.
    pan: Vec2,
}

/// The preview panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    column((toolbar(), stage()))
        .width(percent(100.0))
        .height(percent(100.0))
        .gap(0.0)
        .boxed()
}

/// The zoom, and the resolution rendered at.
fn toolbar() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let theme = cx.theme();
        let pad = theme.space.xl;
        let height = theme.viewport.toolbar;
        let control = theme.space.row;
        cx.build(
            row((
                segmented(
                    ["Fit", "100%"],
                    resource::<PreviewView, _>(|view| {
                        view.zoom as usize
                    }),
                    |world, at| {
                        if let Some(&zoom) = PreviewZoom::ALL.get(at)
                        {
                            world.insert_resource(PreviewView {
                                zoom,
                                pan: Vec2::ZERO,
                            });
                        }
                    },
                )
                .width(px(112.0))
                .height(px(control)),
                frame().grow(1.0),
                label(resource::<ProjectSettings, _>(|settings| {
                    let size = settings.size();
                    format!("{} x {}", size.x, size.y)
                }))
                .tone(Tone::Dim),
            ))
            .width(percent(100.0))
            .height(px(height))
            .shrink(0.0)
            .align(AlignItems::Center)
            .padding(UiRect::horizontal(px(pad))),
        )
    })
}

/// The area the render is shown in, or what stands in for it while
/// the project has no camera.
fn stage() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let image = cx.world.resource::<PreviewImage>().0.clone();
        let node = cx.build(
            column((anchored::<EditorTheme, _>(move |area| {
                keyed::<EditorTheme, bool>(
                    resource::<Rendering, _>(|rendering| rendering.0),
                    move |&rendering| {
                        if rendering {
                            picture(image.clone(), area).boxed()
                        } else {
                            no_camera()
                        }
                    },
                )
                .within(
                    column(())
                        .width(percent(100.0))
                        .height(percent(100.0))
                        .justify(JustifyContent::Center)
                        .align(AlignItems::Center),
                )
            }),))
            .width(percent(100.0))
            .grow(1.0)
            .min_height(px(0.0))
            .overflow(Overflow::clip())
            .with(PreviewPanel),
        );
        cx.world.entity_mut(node).observe(on_drag);
        node
    })
}

/// The render, sized for `area` and the zoom. Hidden until that area
/// has a size: at a fresh `ComputedNode` it does not, and `Auto`
/// would flash at the image's native size for a frame.
fn picture(
    image: Handle<Image>,
    area: Option<Entity>,
) -> impl View<Bevy, EditorTheme> {
    let size = move || {
        changing_under(area, move |world: &World| {
            let zoom = world.resource::<PreviewView>().zoom;
            area.and_then(|area| {
                view::preview_size(world, area, zoom)
            })
        })
    };
    column(())
        .width(size().map(|size| size.map_or(Val::ZERO, |s| px(s.x))))
        .height(
            size().map(|size| size.map_or(Val::ZERO, |s| px(s.y))),
        )
        // Or one larger than the area is squeezed back into it.
        .shrink(0.0)
        .display(size().map(|size| {
            if size.is_some() {
                Display::Flex
            } else {
                Display::None
            }
        }))
        .inset(resource::<PreviewView, _>(|view| UiRect {
            left: px(view.pan.x),
            top: px(view.pan.y),
            ..UiRect::all(Val::Auto)
        }))
        .with(ImageNode::new(image))
}

/// What a project with no camera shows, and the way out.
fn no_camera() -> AnyView<Bevy, EditorTheme> {
    column((
        label("No camera").tone(Tone::Dim),
        button(label("Add camera"))
            .padding(UiRect::axes(px(12.0), px(4.0)))
            .on_activate(hierarchy::spawn_camera),
    ))
    .align(AlignItems::Center)
    .boxed()
}

/// Drags the render around while it is shown at its own size.
fn on_drag(drag: On<Pointer<Drag>>, mut view: ResMut<PreviewView>) {
    if view.zoom == PreviewZoom::Actual {
        view.pan += drag.delta;
    }
}
