//! Dragging a file onto an asset field.
//!
//! What extension loads as which asset is a registration,
//! [`moxie_asset::AssetTypes`]. The drag itself ([`AssetDragging`])
//! carries a path and that same kind, so a drop target only has to
//! compare one [`TypeId`] to know whether what landed on it is its
//! own.

use std::any::TypeId;
use std::path::PathBuf;

use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy_fynix::{Bevy, Cx, Theme, View};

use crate::cursor::PointerEventExt as _;
use crate::drag::{follow, ghost};
use crate::theme::EditorTheme;

/// The file being dragged, its kind, and what is following the cursor
/// meanwhile. Empty whenever nothing is being dragged.
#[derive(Resource, Default)]
pub struct AssetDragging {
    pub path: Option<PathBuf>,
    pub kind: Option<TypeId>,
    ghost: Option<Entity>,
}

/// A view whose root node is a file that can be picked up. See
/// [`draggable`].
pub struct Draggable<V> {
    inner: V,
    path: PathBuf,
    kind: TypeId,
    label: String,
}

/// Makes `inner` a file that can be picked up and dragged onto an
/// asset field, named `label` while it follows the cursor.
pub fn draggable<V>(
    inner: V,
    path: PathBuf,
    kind: TypeId,
    label: String,
) -> Draggable<V> {
    Draggable {
        inner,
        path,
        kind,
        label,
    }
}

impl<V: View<Bevy, EditorTheme>> View<Bevy, EditorTheme>
    for Draggable<V>
{
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let Self {
            inner,
            path,
            kind,
            label,
        } = self;
        let node = inner.build(cx);
        cx.world
            .entity_mut(node)
            .observe(
                move |start: On<Pointer<DragStart>>,
                      theme: Res<Theme<EditorTheme>>,
                      scale: Res<UiScale>,
                      mut dragging: ResMut<AssetDragging>,
                      mut commands: Commands| {
                    if start.button != PointerButton::Primary {
                        return;
                    }

                    let at = start.logical(&scale);

                    dragging.path = Some(path.clone());
                    dragging.kind = Some(kind);
                    dragging.ghost = Some(
                        commands
                            .spawn(ghost(at, label.clone(), &theme.0))
                            .id(),
                    );
                },
            )
            .observe(
                move |drag: On<Pointer<Drag>>,
                      scale: Res<UiScale>,
                      dragging: Res<AssetDragging>,
                      mut nodes: Query<&mut Node>| {
                    let Some(ghost) = dragging.ghost else {
                        return;
                    };
                    let Ok(mut node) = nodes.get_mut(ghost) else {
                        return;
                    };
                    follow(&mut node, drag.logical(&scale));
                },
            )
            .observe(
                move |_: On<Pointer<DragEnd>>,
                      mut dragging: ResMut<AssetDragging>,
                      mut commands: Commands| {
                    if let Some(ghost) = dragging.ghost.take() {
                        commands.entity(ghost).despawn();
                    }
                    dragging.path = None;
                    dragging.kind = None;
                },
            );
        node
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::time::TimePlugin;
    use bevy_fynix::mount;
    use bevy_fynix::views::frame;

    use super::*;
    use crate::MoxieUiPlugin;

    struct Marker;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((TimePlugin, MoxieUiPlugin))
            .init_resource::<UiScale>();
        let node = mount::<EditorTheme>(
            app.world_mut(),
            draggable(
                frame(),
                PathBuf::from("a/b.png"),
                TypeId::of::<Marker>(),
                "b.png".into(),
            ),
        );
        app.update();
        (app, node)
    }

    fn fire<E>(app: &mut App, on: Entity, event: E)
    where
        E: core::fmt::Debug + Clone + Reflect,
        Pointer<E>: bevy::ecs::event::Event,
        for<'t> <Pointer<E> as bevy::ecs::event::Event>::Trigger<'t>:
            Default,
    {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(40.0, 50.0),
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            event,
            on,
        ));
        app.update();
    }

    fn start(app: &mut App, on: Entity, button: PointerButton) {
        let hit = HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
        fire(app, on, DragStart { button, hit });
    }

    fn end(app: &mut App, on: Entity) {
        fire(
            app,
            on,
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
        );
    }

    fn ghost_node(app: &App) -> Option<Entity> {
        app.world().resource::<AssetDragging>().ghost
    }

    #[test]
    fn starting_a_drag_sets_the_resource_and_spawns_the_ghost() {
        let (mut app, node) = app();
        start(&mut app, node, PointerButton::Primary);

        let dragging = app.world().resource::<AssetDragging>();
        assert_eq!(dragging.path, Some(PathBuf::from("a/b.png")));
        assert_eq!(dragging.kind, Some(TypeId::of::<Marker>()));
        let ghost = ghost_node(&app).expect("a ghost");
        let ui = app.world().get::<Node>(ghost).unwrap();
        assert_eq!(ui.left, px(40.0 + crate::drag::GHOST_OFFSET.x));
    }

    #[test]
    fn ending_the_drag_clears_the_resource_and_the_ghost() {
        let (mut app, node) = app();
        start(&mut app, node, PointerButton::Primary);
        let ghost = ghost_node(&app).unwrap();
        end(&mut app, node);

        let dragging = app.world().resource::<AssetDragging>();
        assert_eq!(dragging.path, None);
        assert_eq!(dragging.kind, None);
        assert!(ghost_node(&app).is_none());
        assert!(app.world().get_entity(ghost).is_err());
    }

    #[test]
    fn another_button_does_not_start_a_drag() {
        let (mut app, node) = app();
        start(&mut app, node, PointerButton::Secondary);

        assert!(
            app.world().resource::<AssetDragging>().path.is_none()
        );
        assert!(ghost_node(&app).is_none());
    }
}
