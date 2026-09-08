//! Making an animatable field's label a drag source.
//!
//! Whether a field can be animated is the host's call
//! ([`FieldAnimatable`], set from its scene registry). The drag itself
//! is generic: it carries a [`Field`] and nothing about what dropping
//! it somewhere means.

use bevy::feathers::cursor::EntityCursor;
use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::SystemCursorIcon;

use bevy_fynix::{BevyFynix, WorldEntityMut};
use fynix::composer::Composer;
use fynix::prelude::*;

use super::Field;
use crate::drag::{follow, ghost};
use crate::elements::{Frame, Label};
use crate::reactive::{BevyUi, FynixHost};
use crate::theme::EditorTheme;

/// The host's answer to "can this field be animated?", set from the
/// editor's scene registry. `None` (the default) leaves every field
/// non-draggable.
#[derive(Resource, Default)]
pub struct FieldAnimatable(pub Option<fn(&World, &Field) -> bool>);

impl FieldAnimatable {
    /// Whether `field` is animatable per the host's check.
    pub fn allows(&self, world: &World, field: &Field) -> bool {
        self.0.is_some_and(|check| check(world, field))
    }
}

/// The field dragged out of the inspector and the tag following the
/// cursor. Empty when nothing is being dragged. What a drop does is the
/// host's concern.
#[derive(Resource, Default)]
pub struct DraggedField {
    pub field: Option<Field>,
    ghost: Option<Entity>,
}

/// A field's name in an inspector row: plain text, or - when the field
/// is animatable ([`FieldAnimatable`]) - a keyframe diamond and the
/// text, the pair a drag source that creates an action on the timeline.
pub struct FieldName {
    pub field: Field,
    pub text: String,
    pub size: f32,
    pub color: Color,
    pub bold: bool,
}

impl Composer<FynixHost> for FieldName {
    type Element = Frame;

    fn compose(
        self,
        ui: &mut BevyUi,
    ) -> ElementHandle<FynixHost, Frame> {
        let Self {
            field,
            text,
            size,
            color,
            bold,
        } = self;
        let animatable = ui
            .world
            .resource::<FieldAnimatable>()
            .allows(ui.world, &field);

        let mut row = ui.elem(elem!(
            Frame,
            direction = FlexDirection::Row,
            align = AlignItems::Center,
            column_gap = px(5)
        ));
        if animatable {
            draggable_field(&mut row, field, text.clone());
        }
        row.with(move |ui| {
            if animatable {
                let fill = ui.theme.color.fill;
                ui.elem(elem!(
                    Frame,
                    width = px(6),
                    height = px(6),
                    margin = UiRect::horizontal(px(2)),
                    background = fill,
                ))
                .insert(UiTransform::from_rotation(
                    Rot2::degrees(45.0),
                ));
            }
            ui.elem(elem!(
                Label,
                text = text,
                size = size,
                color = color,
                bold = bold,
                wrap = false
            ));
        })
        .handle()
    }
}

/// Makes `elem` a drag source for `field`, named `label` while it
/// follows the cursor.
fn draggable_field(
    elem: &mut impl WorldEntityMut,
    field: Field,
    label: String,
) {
    elem.insert(EntityCursor::System(SystemCursorIcon::Grab))
        .observe(
            move |start: On<Pointer<DragStart>>,
                  kernel: Res<BevyFynix<EditorTheme>>,
                  scale: Res<UiScale>,
                  mut dragged: ResMut<DraggedField>,
                  mut commands: Commands| {
                if start.button != PointerButton::Primary {
                    return;
                }
                let at = start.pointer_location.position / scale.0;

                dragged.field = Some(field.clone());
                dragged.ghost = Some(
                    commands
                        .spawn(ghost(
                            at,
                            label.clone(),
                            kernel.theme(),
                        ))
                        .id(),
                );
            },
        )
        .observe(
            move |drag: On<Pointer<Drag>>,
                  scale: Res<UiScale>,
                  dragged: Res<DraggedField>,
                  mut nodes: Query<&mut Node>| {
                let Some(ghost) = dragged.ghost else {
                    return;
                };
                let Ok(mut node) = nodes.get_mut(ghost) else {
                    return;
                };
                follow(
                    &mut node,
                    drag.pointer_location.position / scale.0,
                );
            },
        )
        .observe(
            move |_: On<Pointer<DragEnd>>,
                  mut dragged: ResMut<DraggedField>,
                  mut commands: Commands| {
                if let Some(ghost) = dragged.ghost.take() {
                    commands.entity(ghost).despawn();
                }
                dragged.field = None;
            },
        );
}
