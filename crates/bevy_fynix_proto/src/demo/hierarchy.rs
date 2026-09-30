//! A hierarchy row: a foldable whose header button names an entity
//! and selects it.

use bevy_color::Color;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::name::Name;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use bevy_ui::{JustifyContent, percent, px};

use crate::prop::derived;
use crate::tokens::{SpacingTokens, SurfaceTokens, TextTokens, Tone};
use crate::views::{BehaviorExt, Tagged, button, foldable, label};
use crate::{Bevy, View};

/// The entity the editor has selected.
#[derive(Resource, Default)]
pub struct Selected(pub Option<Entity>);

/// On a header whose entity can be dragged. A stand-in that only
/// records the attachment.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragSource(pub Entity);

/// On a header with a context menu. A stand-in that only records the
/// attachment.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextMenu(pub &'static str);

/// Stand-ins for the drag and context menu modifiers.
pub trait RowExt: Sized {
    fn draggable(self, entity: Entity) -> Tagged<Self, DragSource> {
        self.tagged(DragSource(entity))
    }

    fn context_menu(
        self,
        menu: &'static str,
    ) -> Tagged<Self, ContextMenu> {
        self.tagged(ContextMenu(menu))
    }
}

impl<V> RowExt for V {}

const UNNAMED: &str = "(unnamed)";

fn name_of(world: &World, entity: Entity) -> String {
    match world.get::<Name>(entity) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => UNNAMED.to_string(),
    }
}

fn is_unnamed(world: &World, entity: Entity) -> bool {
    world.get::<Name>(entity).is_none_or(|name| name.is_empty())
}

fn select(world: &mut World, entity: Entity) {
    world.resource_mut::<Selected>().0 = Some(entity);
}

/// The row for `entity`, filled with `selected` while it is the
/// selection, with `body` folded under it.
pub fn hierarchy_row<T>(
    entity: Entity,
    selected: Color,
    body: impl View<Bevy, T>,
) -> impl View<Bevy, T>
where
    T: TextTokens
        + SurfaceTokens
        + SpacingTokens
        + Send
        + Sync
        + 'static,
{
    let header = button(
        label(derived(move |world| name_of(world, entity)))
            .wrap(false)
            .tone(derived(move |world| {
                if is_unnamed(world, entity) {
                    Tone::Dim
                } else {
                    Tone::Body
                }
            })),
    )
    .width(percent(100.0))
    .height(px(18.0))
    .justify(JustifyContent::FlexStart)
    .fill(derived(move |world| {
        // A signal cannot read the theme: `Theme<T>` is out of the
        // world while bound props are re-read.
        if world.resource::<Selected>().0 == Some(entity) {
            selected
        } else {
            Color::NONE
        }
    }))
    .on_activate(move |world| select(world, entity))
    .draggable(entity)
    .context_menu("row");

    foldable(header, body)
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use bevy_ui::{Display, Val};
    use bevy_ui_widgets::{Activate, Button as ButtonBehavior};

    use super::*;
    use crate::demo::testing::{
        BODY, DIM, Demo, HOVER, app, color, fill, kids, text, ui,
    };
    use crate::views::Open;
    use crate::{AnyView, mount};

    /// Every part of the built row.
    struct Row {
        root: Entity,
        chevron: Entity,
        header: Entity,
        name: Entity,
        body: Entity,
    }

    fn build(app: &mut App, entity: Entity, body: &str) -> Row {
        app.init_resource::<Selected>();
        let body = body.to_string();
        let root = mount::<Demo>(
            app.world_mut(),
            hierarchy_row::<Demo>(
                entity,
                HOVER,
                AnyView::new(move |cx| {
                    cx.build(label(body.as_str()))
                }),
            ),
        );
        let [top, body] = kids(app, root)[..] else {
            panic!("a header row and a body");
        };
        let [chevron, header] = kids(app, top)[..] else {
            panic!("a chevron and a header");
        };
        let name = kids(app, header)[0];
        Row {
            root,
            chevron,
            header,
            name,
            body,
        }
    }

    fn subject(app: &mut App, name: &str) -> Entity {
        app.world_mut().spawn(Name::new(name.to_string())).id()
    }

    #[test]
    fn the_header_is_a_button_holding_the_name() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let row = build(&mut app, entity, "child");

        assert!(
            app.world().get::<ButtonBehavior>(row.header).is_some()
        );
        assert_eq!(text(&app, row.name), "Cube");
        assert_eq!(ui(&app, row.header).height, Val::Px(18.0));
        assert_eq!(text(&app, kids(&app, row.body)[0]), "child");
    }

    #[test]
    fn the_stand_ins_only_record_they_were_attached() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let row = build(&mut app, entity, "child");

        assert_eq!(
            app.world().get::<DragSource>(row.header),
            Some(&DragSource(entity))
        );
        assert_eq!(
            app.world().get::<ContextMenu>(row.header),
            Some(&ContextMenu("row"))
        );
    }

    #[test]
    fn the_name_follows_the_component_and_dims_while_empty() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let row = build(&mut app, entity, "child");
        assert_eq!(color(&app, row.name), BODY);

        app.world_mut().entity_mut(entity).insert(Name::new(""));
        app.update();
        assert_eq!(text(&app, row.name), UNNAMED);
        assert_eq!(color(&app, row.name), DIM);

        app.world_mut()
            .entity_mut(entity)
            .insert(Name::new("Sphere"));
        app.update();
        assert_eq!(text(&app, row.name), "Sphere");
        assert_eq!(color(&app, row.name), BODY);

        app.world_mut().entity_mut(entity).remove::<Name>();
        app.update();
        assert_eq!(color(&app, row.name), DIM);
    }

    #[test]
    fn activating_the_header_selects_and_fills_it() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let other = subject(&mut app, "Other");
        let row = build(&mut app, entity, "child");
        let other_row = build(&mut app, other, "x");
        assert_eq!(fill(&app, row.header), Color::NONE);

        app.world_mut().trigger(Activate { entity: row.header });
        app.update();

        assert_eq!(
            app.world().resource::<Selected>().0,
            Some(entity)
        );
        assert_eq!(fill(&app, row.header), HOVER);
        assert_eq!(fill(&app, other_row.header), Color::NONE);
    }

    #[test]
    fn the_chevron_folds_the_body() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let row = build(&mut app, entity, "child");
        let glyph = kids(&app, row.chevron)[0];
        assert!(app.world().get::<Open>(row.root).is_some());
        assert_eq!(ui(&app, row.body).display, Display::Flex);
        assert_eq!(text(&app, glyph), "v");

        app.world_mut().trigger(Activate {
            entity: row.chevron,
        });
        app.update();
        assert!(app.world().get::<Open>(row.root).is_none());
        assert_eq!(ui(&app, row.body).display, Display::None);
        assert_eq!(text(&app, glyph), ">");
        assert!(
            app.world().resource::<Selected>().0.is_none(),
            "folding does not select"
        );

        app.world_mut().trigger(Activate {
            entity: row.chevron,
        });
        app.update();
        assert_eq!(ui(&app, row.body).display, Display::Flex);
    }

    #[test]
    fn a_state_component_set_from_outside_folds_too() {
        let mut app = app();
        let entity = subject(&mut app, "Cube");
        let row = build(&mut app, entity, "child");

        app.world_mut().entity_mut(row.root).remove::<Open>();
        app.update();

        assert_eq!(ui(&app, row.body).display, Display::None);
    }
}
