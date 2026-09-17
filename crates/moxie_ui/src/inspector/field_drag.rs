//! Making an animatable field's label a drag source.
//!
//! Whether a field can be animated is the host's call
//! ([`FieldAnimatable`], set from its scene registry). The drag itself
//! is generic: it carries a [`Field`] and nothing about what dropping
//! it somewhere means.

use std::collections::HashSet;

use bevy::feathers::cursor::EntityCursor;
use bevy::picking::events::{
    Click, Drag, DragEnd, DragStart, Pointer,
};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::SystemCursorIcon;

use bevy_fynix::{BevyFynix, WorldEntityMut};
use fynix::composer::Composer;
use fynix::prelude::*;

use super::{Field, Source};
use crate::drag::{follow, ghost};
use crate::elements::{Diamond, DiamondCursor, Frame, Label};
use crate::reactive::{BevyUi, FynixHost, value_changed};
use crate::theme::EditorTheme;

/// The axis names every registered vector [`Inspect`](super::Inspect)
/// widget breaks its own field into - see `vector.rs`'s `Axes::NAMES`.
const AXIS_NAMES: [&str; 4] = ["x", "y", "z", "w"];

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

/// The host's answer to "does this field already drive an action?",
/// set from the editor's scene tree. `None` (the default) never marks
/// a field as already animated.
#[derive(Resource, Default)]
pub struct FieldHasAction(pub Option<fn(&World, &Field) -> bool>);

impl FieldHasAction {
    /// Whether `field` already has an action, per the host's check.
    pub fn check(&self, world: &World, field: &Field) -> bool {
        self.0.is_some_and(|check| check(world, field))
    }
}

/// The host's way to reach `field`'s own stage entry, set from the
/// editor's scene tree. `None` (the default) never offers one, so no
/// diamond can be toggled into stage-edit mode.
#[derive(Resource, Default)]
pub struct FieldStageSource(
    pub Option<fn(&World, &Field) -> Option<Box<dyn Source>>>,
);

impl FieldStageSource {
    /// `field`'s stage entry as a [`Source`], per the host's check.
    /// `None` when the host offers none, or the field has no entry to
    /// show.
    pub fn resolve(
        &self,
        world: &World,
        field: &Field,
    ) -> Option<Box<dyn Source>> {
        self.0.and_then(|resolve| resolve(world, field))
    }
}

/// Fields whose diamond is toggled to show and edit their stage entry
/// ([`FieldStageSource`]) instead of their live value.
#[derive(Resource, Default)]
pub struct StagedFieldEdit(HashSet<Field>);

impl StagedFieldEdit {
    pub fn is_active(&self, field: &Field) -> bool {
        self.0.contains(field)
    }

    fn set(&mut self, field: Field, active: bool) {
        if active {
            self.0.insert(field);
        } else {
            self.0.remove(&field);
        }
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
        let has_action = |world: &World, field: &Field| {
            world.resource::<FieldHasAction>().check(world, field)
        };
        let staged = |world: &World, field: &Field| {
            world.resource::<StagedFieldEdit>().is_active(field)
        };
        let accent = ui.theme.color.accent;
        let stage = ui.theme.color.stage;
        let neutral = ui.theme.color.fill;
        let diamond_fill =
            move |world: &World, field: &Field| -> Color {
                if staged(world, field) {
                    stage
                } else if has_action(world, field) {
                    accent
                } else {
                    neutral
                }
            };

        let mut row = ui.elem(elem!(
            Frame,
            direction = FlexDirection::Row,
            align = AlignItems::Center,
            column_gap = px(5)
        ));
        // `field` moves into the drag wiring below; these clones let
        // the diamond keep re-checking its own state on every later
        // poll.
        let diamond_field = field.clone();
        let bind_field = field.clone();
        let click_field = field.clone();
        if animatable {
            draggable_field(&mut row, field, text.clone());
        }
        row.with(move |ui| {
            if animatable {
                let fill = diamond_fill(ui.world, &diamond_field);
                ui.elem(elem!(Diamond, background = fill))
                    .observe(
                    move |mut click: On<Pointer<Click>>,
                          mut commands: Commands| {
                        click.propagate(false);
                        let field = click_field.clone();
                        commands.queue(move |world: &mut World| {
                            toggle_staged_edit(world, &field);
                        });
                    },
                )
                .bind(
                    |diamond| diamond.background(),
                    value_changed(move |world, _| {
                        (
                            has_action(world, &bind_field),
                            staged(world, &bind_field),
                        )
                    }),
                    move |WorldNodeRef { world, .. }| {
                        diamond_fill(world, &diamond_field)
                    },
                );
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

/// Toggles `field`'s own stage-edit state, and cascades the same new
/// state onto whichever of [`AXIS_NAMES`] under it also has an
/// action - a compound field's diamond turns its staged axes with it.
fn toggle_staged_edit(world: &mut World, field: &Field) {
    let has_action = |world: &World, field: &Field| {
        world.resource::<FieldHasAction>().check(world, field)
    };
    if !has_action(world, field) {
        return;
    }

    let active =
        !world.resource::<StagedFieldEdit>().is_active(field);
    world
        .resource_mut::<StagedFieldEdit>()
        .set(field.clone(), active);

    for name in AXIS_NAMES {
        let child = field.child(name);
        if has_action(world, &child) {
            world
                .resource_mut::<StagedFieldEdit>()
                .set(child, active);
        }
    }
}

/// Makes `elem` a drag source for `field`, named `label` while it
/// follows the cursor.
pub(crate) fn draggable_field(
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
