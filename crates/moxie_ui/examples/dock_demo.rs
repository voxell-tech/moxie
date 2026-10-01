//! Demonstrates the docking system in [`moxie_ui::widgets::dock`].
//!
//! Three trivial panels ("Panel A/B/C") start as tabs in one
//! full-window area. Try:
//! - dragging a tab left/right within the tab bar to reorder it,
//! - dragging a tab onto another area's tab bar to merge it in,
//! - dragging a tab onto an area's top/bottom/left/right edge to
//!   split,
//! - dragging the divider between two areas to resize them,
//! - pressing Escape mid-drag to cancel.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps, column, label};
use bevy_fynix::{AnyView, Bevy, ViewExt, mount};
use moxie_ui::MoxieUiPlugin;
use moxie_ui::theme::EditorTheme;
use moxie_ui::widgets::dock::{
    DockAreaStyle, DockLeaf, DockRegistry, DockTree, DockWindowKind,
    dock,
};

fn main() {
    App::new()
        .add_plugins((
            // `../assets`: the editor crates share one asset folder
            // (`editor/assets`) rather than each carrying its own.
            DefaultPlugins.set(AssetPlugin {
                file_path: "../assets".into(),
                ..default()
            }),
            MoxieUiPlugin,
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(world: &mut World) {
    world.spawn(Camera2d);

    let mut registry =
        world.resource_mut::<DockRegistry<EditorTheme>>();
    for (id, name) in [
        ("panel_a", "Panel A"),
        ("panel_b", "Panel B"),
        ("panel_c", "Panel C"),
    ] {
        registry.register(
            id,
            DockWindowKind::new(name, move || panel(name)),
        );
    }

    // One root leaf holding all three panels as tabs.
    world.resource_mut::<DockTree>().set_root_leaf(
        DockLeaf::new("root", DockAreaStyle::TabBar).with_windows(
            vec![
                "panel_a".into(),
                "panel_b".into(),
                "panel_c".into(),
            ],
        ),
    );

    mount::<EditorTheme>(
        world,
        column((dock::<EditorTheme>(),))
            .width(percent(100.0))
            .height(percent(100.0)),
    );
}

/// A panel's whole content: its name.
fn panel(name: &'static str) -> AnyView<Bevy, EditorTheme> {
    label(name).size(20.0).boxed()
}
