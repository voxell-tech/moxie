//! Dragging a file onto an asset field.
//!
//! What extension loads as which asset is a registration,
//! [`moxie_asset::AssetKinds`]. The drag itself ([`AssetDragging`])
//! carries a path and that same kind, so a drop target only has to
//! compare one [`TypeId`] to know whether what landed on it is its
//! own.

use std::any::TypeId;
use std::path::PathBuf;

use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;

use crate::reactive::BevyFynix;
use bevy_fynix::WorldEntityMut;
use fynix::element::Element;
use fynix::ui::ElementMut;

use crate::cursor::PointerEventExt as _;
use crate::drag::{follow, ghost};
use crate::reactive::FynixHost;

/// The file being dragged, its kind, and what is following the cursor
/// meanwhile. Empty whenever nothing is being dragged.
#[derive(Resource, Default)]
pub struct AssetDragging {
    pub path: Option<PathBuf>,
    pub kind: Option<TypeId>,
    ghost: Option<Entity>,
}

/// Makes `elem` a file that can be picked up and dragged onto an
/// asset field, named `label` while it follows the cursor.
pub fn draggable<'r, 'u, 'a, E: Element<FynixHost>>(
    elem: &'r mut ElementMut<'u, 'a, FynixHost, E>,
    path: PathBuf,
    kind: TypeId,
    label: String,
) -> &'r mut ElementMut<'u, 'a, FynixHost, E> {
    elem.observe(
        move |start: On<Pointer<DragStart>>,
              kernel: Res<BevyFynix>,
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
                    .spawn(ghost(at, label.clone(), kernel.theme()))
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
    )
}
