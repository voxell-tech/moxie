//! The menu bar above the dock, holding what acts on the project as a
//! whole rather than on anything a panel is showing.

use bevy::prelude::*;
use bevy::ui_widgets::{
    ActivateOnPress, MenuButton as MenuButtonBehavior,
};
use bevy_fynix::WorldEntityMut;
use fynix::composer::Composer;
use fynix::prelude::*;
use moxie_ui::elements::{
    Dropdown, DropdownList, DropdownMenu, Frame, Label, MenuButton,
    menu_item,
};
use moxie_ui::reactive::{BevyUi, FynixHost};

use crate::project;

pub(super) struct TopBar;

impl Composer<FynixHost> for TopBar {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        ui.elem(elem!(
            Frame,
            width = percent(100),
            direction = FlexDirection::Row,
            align = AlignItems::Center,
        ))
        .with(|ui| {
            ui.compose(Menu {
                name: "File",
                entries: vec![
                    ("New", project::new_scene),
                    ("Open", project::load_scene),
                    ("Save", project::save_scene),
                ],
            });
        })
        .handle()
    }
}

/// One menu: the name in the bar, and what picking an entry runs.
struct Menu {
    name: &'static str,
    entries: Vec<(&'static str, fn(&mut World))>,
}

impl Composer<FynixHost> for Menu {
    type Element = DropdownMenu;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, DropdownMenu> {
        let Self { name, entries } = self;
        // Sized to the longest entry, so the list clears its own text
        // whichever menu it belongs to.
        let width = Dropdown::width_for(
            &entries
                .iter()
                .map(|(entry, _)| entry.to_string())
                .collect::<Vec<_>>(),
            12.0,
        );

        ui.elem(elem!(DropdownMenu))
            .with(move |ui| {
                title(ui, name);

                ui.elem(elem!(DropdownList, width = width)).with(
                    move |ui| {
                        for (entry, run) in entries {
                            menu_item(ui, None, entry, run);
                        }
                    },
                );
            })
            .handle()
    }
}

/// The name in the bar, which opens the menu.
///
/// A button: an entry in a menu bar is a word, so it wears no
/// chevron.
fn title(ui: &mut BevyUi, name: &str) {
    let text = ui.theme.color.text;
    ui.elem(elem!(
        !MenuButton,
        label = elem!(
            Label,
            text = name.to_string(),
            wrap = false,
            color = text
        )
    ))
    // What the menu's own observer reaches this through to open the
    // list beneath it.
    .insert((MenuButtonBehavior, ActivateOnPress));
}
