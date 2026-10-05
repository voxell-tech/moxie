//! The bar over a viewport: the gizmo's mode and space, the grid, and
//! where the camera looks from.

use bevy::prelude::*;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, TooltipExt as _, button,
    dropdown, frame, ghost, label, row, segmented,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, component, resource,
};
use moxie_ui::theme::EditorTheme;

use super::camera::{EditorCamera, View, show};
use super::gizmo::{GizmoMode, GizmoSettings, GizmoSpace};

/// The toolbar of the viewport `camera` draws.
pub(super) fn toolbar(camera: Entity) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let theme = cx.theme();
        let pad = theme.space.xl;
        let gap = theme.space.lg;
        let height = theme.viewport.toolbar;
        let control = theme.space.row;
        let lit = theme.color.selection;

        let mode = segmented(
            ["Move", "Rotate", "Scale"],
            resource::<GizmoSettings, _>(|settings| {
                settings.mode as usize
            }),
            |world, at| {
                if let Some(&mode) = GizmoMode::ALL.get(at) {
                    world.resource_mut::<GizmoSettings>().mode = mode;
                }
            },
        )
        .width(px(168.0))
        .height(px(control))
        .tooltip(|| label("Gizmo mode (W, E, R)"));
        let space = segmented(
            ["World", "Local"],
            resource::<GizmoSettings, _>(|settings| {
                settings.space as usize
            }),
            |world, at| {
                if let Some(&space) = GizmoSpace::ALL.get(at) {
                    world.resource_mut::<GizmoSettings>().space =
                        space;
                }
            },
        )
        .width(px(112.0))
        .height(px(control));
        let grid = button(label("Grid"))
            .height(px(control))
            .padding(UiRect::horizontal(px(pad)))
            .fill(component::<EditorCamera, _>(
                camera,
                move |orbit| {
                    if orbit.is_some_and(|orbit| orbit.grid) {
                        lit
                    } else {
                        Color::NONE
                    }
                },
            ))
            .rules(ghost)
            .on_activate(move |world| {
                if let Some(mut orbit) =
                    world.get_mut::<EditorCamera>(camera)
                {
                    orbit.grid = !orbit.grid;
                }
            });
        let chevron = cx
            .world
            .resource::<AssetServer>()
            .load(moxie_ui::icons::CHEVRON);
        let view = dropdown(
            View::ALL.map(View::label),
            component::<EditorCamera, _>(camera, |orbit| {
                let view = orbit.map(EditorCamera::view);
                View::ALL
                    .iter()
                    .position(|&listed| Some(listed) == view)
                    .unwrap_or(0)
            }),
            chevron,
            move |world, at| {
                if let Some(&view) = View::ALL.get(at) {
                    let ran = world
                        .run_system_cached_with(show, (camera, view));
                    if let Err(err) = ran {
                        error!("could not change the view: {err}");
                    }
                }
            },
        )
        .width(px(132.0))
        .max_width(px(132.0))
        .height(px(control))
        .tooltip(|| label("View (numpad 5, 1, 3, 7, 0)"));

        cx.build(
            row((mode, space, frame().grow(1.0), grid, view))
                .width(percent(100.0))
                .height(px(height))
                .shrink(0.0)
                .align(AlignItems::Center)
                .gap(gap)
                .padding(UiRect::horizontal(px(pad))),
        )
    })
}
