//! The settings panel: a reflect inspector over [`EditorSettings`],
//! and the button that writes it back to disk.

use bevy::ecs::system::Command as _;
use bevy::prelude::*;
use bevy::settings::SaveSettingsSync;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, label, row, scroll,
};
use bevy_fynix::{AnyView, Bevy};
use moxie_ui::elements::resource_inspector_of;
use moxie_ui::theme::EditorTheme;

use crate::EditorSettings;

/// The settings panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        cx.build(
            scroll((
                resource_inspector_of::<EditorSettings>(),
                save_row(),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .gap(8.0)
            .padding(UiRect::all(px(pad))),
        )
    })
}

/// The one action the panel has of its own.
fn save_row() -> impl bevy_fynix::View<Bevy, EditorTheme> {
    row((button(label("Save"))
        .width(px(64.0))
        .height(px(24.0))
        .on_activate(|world| {
            SaveSettingsSync::Always.apply(world);
        }),))
}
