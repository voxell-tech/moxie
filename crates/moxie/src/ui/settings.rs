//! The project panel: a reflect inspector over [`ProjectSettings`],
//! which the project file keeps.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, scroll};
use bevy_fynix::{AnyView, Bevy};
use moxie_ui::elements::resource_inspector_of;
use moxie_ui::theme::EditorTheme;

use crate::ProjectSettings;

/// The project panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        cx.build(
            scroll((resource_inspector_of::<ProjectSettings>(),))
                .width(percent(100.0))
                .height(percent(100.0))
                .padding(UiRect::all(px(pad))),
        )
    })
}
