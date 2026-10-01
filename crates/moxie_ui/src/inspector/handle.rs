//! [`Inspect`] for a [`Handle<T>`], for any asset `T`.
//!
//! One row, showing whatever asset is currently assigned. Clicking it
//! opens the [asset picker](crate::asset_picker), and a file dragged
//! from the assets panel whose registered [`moxie_asset::AssetTypes`]
//! kind matches `T` can be dropped on it.

use std::any::TypeId;

use bevy::asset::{Asset, AssetPath};
use bevy::picking::events::{DragDrop, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, ghost, icon, label,
    row,
};
use bevy_fynix::{AnyView, Bevy, ScopedExt as _, ViewExt as _};
use moxie_asset::{ABSOLUTE_SOURCE, AssetRef, asset_choices};

use super::{Binding, Inspect};
use crate::asset::AssetDragging;
use crate::asset_picker::open_asset_picker;
use crate::cursor::Cursor;
use crate::gaps::changing;
use crate::icons;
use crate::theme::EditorTheme;

impl<T: Asset + TypePath> Inspect for Handle<T> {
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
        AnyView::<Bevy, EditorTheme>::new(move |cx| {
            let asset = cx
                .world
                .resource::<AssetServer>()
                .load::<Image>(icons::ASSET);
            // The label names what is held by its choice's name, so
            // it follows the list of choices as well as
            // the handle.
            let named = binding.clone();
            let shown = changing(move |world: &World| {
                label_of::<T>(world, &named)
            });

            let picked = binding.clone();
            let slot = cx.build(
                button(
                    row((
                        icon(asset).tone(Tone::Dim),
                        label(shown).tone(Tone::Dim).wrap(false),
                    ))
                    .width(percent(100.0))
                    .align(AlignItems::Center)
                    .justify(JustifyContent::SpaceBetween),
                )
                .width(percent(100.0))
                .rules(ghost)
                .on_activate(move |world| {
                    let at = cursor_position(world);
                    open_asset_picker::<T>(world, at, picked.clone());
                }),
            );
            accept_drop::<T>(cx.world, slot, binding);
            slot
        })
        .boxed()
    }
}

/// Where the pointer is, in logical screen space. The corner when
/// there is none, as when the row is pressed from the keyboard, where
/// placement pushes the picker on screen.
fn cursor_position(world: &mut World) -> Vec2 {
    world
        .run_system_cached(|cursor: Cursor| cursor.position())
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Makes `slot` load a file dragged from the assets panel into
/// `binding`, when its registered kind is `T`.
fn accept_drop<T: Asset>(
    world: &mut World,
    slot: Entity,
    binding: Binding,
) {
    let kind = TypeId::of::<T>();

    world.entity_mut(slot).observe(
        move |drop: On<Pointer<DragDrop>>,
              dragging: Res<AssetDragging>,
              mut commands: Commands| {
            if drop.button != PointerButton::Primary {
                return;
            }
            let (Some(path), Some(dragged)) =
                (dragging.path.clone(), dragging.kind)
            else {
                return;
            };
            if dragged != kind {
                return;
            }

            let binding = binding.clone();
            commands.queue(move |world: &mut World| {
                // Rooted at `/`: a dragged path is absolute and may
                // live anywhere on disk.
                let asset_path =
                    AssetPath::from_path_buf(path.clone())
                        .with_source(ABSOLUTE_SOURCE);
                // A dragged file's path is outside the configured
                // asset root by construction, so it needs `Deny`'s
                // per-load override; see `unapproved_path_mode` in
                // `main.rs`.
                let handle = world
                    .resource::<AssetServer>()
                    .load_builder()
                    .override_unapproved()
                    .load::<T>(asset_path);
                binding.write(world, handle);
            });
        },
    );
}

/// What `binding` currently holds: the name of the [`asset_choices`]
/// entry it matches, else the asset's own path, or a placeholder for
/// a handle that names nothing.
fn label_of<T: Asset + TypePath>(
    world: &World,
    binding: &Binding,
) -> String {
    let Some(asset) = binding
        .read::<Handle<T>>(world)
        .zip(world.get_resource::<AssetServer>())
        .and_then(|(handle, assets)| AssetRef::of(&handle, assets))
    else {
        return "(none)".to_string();
    };

    let name = asset_choices::<T>(world)
        .find(|choice| choice.asset == asset)
        .map(|choice| choice.name.clone());
    match (name, asset) {
        (Some(name), _) => name,
        (None, AssetRef::Path(path)) => path,
        (None, AssetRef::Uuid(_)) => "(unnamed)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use bevy::asset::uuid::Uuid;
    use bevy::ui::widget::Text;
    use bevy::ui_widgets::{Activate, Button};
    use moxie_asset::{AssetChoice, FoundAssets, type_data};

    use super::*;
    use crate::asset_picker::AssetPickerRoot;
    use crate::inspector::{Field, ReflectInspect};
    use crate::tests;

    /// A field holding an image, for the row to edit.
    #[derive(Component, Reflect, Default)]
    #[reflect(Component, Default)]
    struct Holder {
        image: Handle<Image>,
    }

    fn uuid(id: u128) -> Handle<Image> {
        Handle::from(Uuid::from_u128(id))
    }

    fn found(app: &mut App, name: &str, id: u128) {
        app.world_mut().resource_mut::<FoundAssets>().0.insert(
            TypeId::of::<Image>(),
            vec![AssetChoice {
                name: name.to_string(),
                asset: AssetRef::Uuid(Uuid::from_u128(id)),
                group: String::new(),
            }],
        );
    }

    /// An app with a holder of an image and the row editing it.
    fn setup() -> (App, Entity, Entity) {
        let mut app = tests::app();
        app.register_type::<Holder>();
        let holder = app.world_mut().spawn(Holder::default()).id();
        let binding =
            Binding::from(Field::of::<Holder>(holder).child("image"));
        let root =
            tests::show(&mut app, Handle::<Image>::build(binding));
        (app, holder, root)
    }

    fn shown(app: &App, root: Entity) -> String {
        let text = tests::all::<Text>(app, root)[0];
        app.world().get::<Text>(text).unwrap().0.clone()
    }

    fn held(app: &App, holder: Entity) -> Handle<Image> {
        app.world().get::<Holder>(holder).unwrap().image.clone()
    }

    #[test]
    fn the_four_handle_types_are_registered() {
        let app = tests::app();
        let world = app.world();
        for kind in [
            TypeId::of::<Handle<StandardMaterial>>(),
            TypeId::of::<Handle<Mesh>>(),
            TypeId::of::<Handle<ColorMaterial>>(),
            TypeId::of::<Handle<Font>>(),
        ] {
            assert!(
                type_data::<ReflectInspect>(world, kind).is_some()
            );
        }
    }

    #[test]
    fn the_default_handle_is_unnamed() {
        let (app, _, root) = setup();

        assert_eq!(shown(&app, root), "(unnamed)");
    }

    #[test]
    fn a_field_that_cannot_be_read_reads_none() {
        let mut app = tests::app();
        app.register_type::<Holder>();
        let nowhere = app.world_mut().spawn_empty().id();
        let binding = Binding::from(
            Field::of::<Holder>(nowhere).child("image"),
        );
        let root =
            tests::show(&mut app, Handle::<Image>::build(binding));

        assert_eq!(shown(&app, root), "(none)");
    }

    #[test]
    fn the_row_names_the_choice_the_handle_matches() {
        let (mut app, holder, root) = setup();
        found(&mut app, "Sky", 2);
        app.world_mut().get_mut::<Holder>(holder).unwrap().image =
            uuid(2);
        app.update();
        assert_eq!(shown(&app, root), "Sky");

        found(&mut app, "Sunset", 2);
        app.update();
        assert_eq!(
            shown(&app, root),
            "Sunset",
            "follows the choices"
        );

        found(&mut app, "Other", 3);
        app.update();
        assert_eq!(shown(&app, root), "(unnamed)");
    }

    #[test]
    fn activating_the_row_opens_the_picker_on_this_field() {
        let (mut app, holder, root) = setup();
        found(&mut app, "Sky", 2);

        app.world_mut().trigger(Activate { entity: root });
        app.update();

        let picker = app
            .world_mut()
            .query_filtered::<Entity, With<AssetPickerRoot>>()
            .iter(app.world())
            .next()
            .expect("a picker");
        let sky = tests::all::<Button>(&app, picker)
            .into_iter()
            .find(|button| {
                tests::all::<Text>(&app, *button).iter().any(|text| {
                    app.world().get::<Text>(*text).unwrap().0 == "Sky"
                })
            })
            .expect("a Sky cell");
        tests::click(&mut app, sky, PointerButton::Primary, 1);
        assert_eq!(held(&app, holder), uuid(2));
    }

    fn dragging(app: &mut App, kind: TypeId) {
        let mut dragging =
            app.world_mut().resource_mut::<AssetDragging>();
        dragging.path = Some("/tmp/pic.png".into());
        dragging.kind = Some(kind);
    }

    #[test]
    fn a_dropped_file_of_the_same_kind_is_loaded_into_the_field() {
        let (mut app, holder, root) = setup();
        dragging(&mut app, TypeId::of::<Image>());

        tests::drop_on(&mut app, root);

        let handle = held(&app, holder);
        let path = app
            .world()
            .resource::<AssetServer>()
            .get_path(&handle)
            .expect("a loaded file");
        assert!(path.to_string().contains("pic.png"));
    }

    #[test]
    fn a_dropped_file_of_another_kind_is_ignored() {
        let (mut app, holder, root) = setup();
        dragging(&mut app, TypeId::of::<Mesh>());

        tests::drop_on(&mut app, root);

        assert_eq!(held(&app, holder), Handle::default());
    }
}
