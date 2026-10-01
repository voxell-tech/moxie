//! Making an animatable field's label a drag source.
//!
//! Whether a field can be animated is the host's call
//! ([`FieldAnimatable`], set from its scene registry). The drag itself
//! is generic: it carries a [`Field`] and nothing about what dropping
//! it somewhere means.

use bevy::picking::events::{Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::UiScale;
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{TextTokens as _, Tone};
use bevy_fynix::views::{HasAction, label};
use bevy_fynix::{Bevy, Cx, EntityCursor, Prop, Theme, View};

use super::Field;
use crate::cursor::PointerEventExt as _;
use crate::drag::{follow, ghost};
use crate::gaps::{changing, set_ink};
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

/// The field dragged out of the inspector and the tag following the
/// cursor. Empty when nothing is being dragged. What a drop does is the
/// host's concern.
#[derive(Resource, Default)]
pub struct DraggedField {
    pub field: Option<Field>,
    ghost: Option<Entity>,
}

/// A field's name in an inspector row: a plain label, or - when the
/// field is animatable ([`FieldAnimatable`]) - a label that is also a
/// drag source creating an action on the timeline, and turns the
/// accent tone while the field already has one ([`FieldHasAction`]).
///
/// With no field, as for a value the editor keeps elsewhere, it is
/// always a plain label.
pub struct FieldName {
    field: Option<Field>,
    text: String,
    tone: Tone,
    ink: Option<Color>,
    bold: bool,
}

/// The name of `field`, shown as `text`.
pub fn field_name(
    field: Option<Field>,
    text: impl Into<String>,
) -> FieldName {
    FieldName {
        field,
        text: text.into(),
        tone: Tone::Body,
        ink: None,
        bold: false,
    }
}

impl FieldName {
    /// The label's tone, body when unset.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// A colour for the label instead of a tone.
    pub fn ink(mut self, ink: Color) -> Self {
        self.ink = Some(ink);
        self
    }

    pub fn bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }
}

impl View<Bevy, EditorTheme> for FieldName {
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let theme = cx.theme();
        let base = self.ink.unwrap_or_else(|| theme.tone(self.tone));
        let accent = theme.tone(Tone::Accent);

        let node = cx.build(
            label(self.text.as_str())
                .tone(self.tone)
                .bold(self.bold)
                .wrap(false),
        );
        if let Some(ink) = self.ink {
            set_ink(cx.world, node, ink);
        }

        let Some(field) = self.field.filter(|field| {
            cx.world
                .resource::<FieldAnimatable>()
                .allows(cx.world, field)
        }) else {
            return node;
        };

        let probe = field.clone();
        cx.effect(
            node,
            Prop::from(changing(move |world: &World| {
                world
                    .resource::<FieldHasAction>()
                    .check(world, &probe)
            })),
            move |world, node, &has_action| {
                if let Ok(mut entity) = world.get_entity_mut(node) {
                    if has_action {
                        entity.insert(HasAction);
                    } else {
                        entity.remove::<HasAction>();
                    }
                }
                set_ink(
                    world,
                    node,
                    if has_action { accent } else { base },
                );
            },
        );
        draggable(cx.world, node, field, self.text);
        node
    }
}

/// A view whose root node is a drag source for a field. See
/// [`draggable_field`].
pub struct DraggableField<V> {
    inner: V,
    field: Field,
    label: String,
}

/// Makes `inner` a drag source for `field`, named `label` while it
/// follows the cursor.
pub fn draggable_field<V>(
    inner: V,
    field: Field,
    label: String,
) -> DraggableField<V> {
    DraggableField {
        inner,
        field,
        label,
    }
}

impl<V: View<Bevy, EditorTheme>> View<Bevy, EditorTheme>
    for DraggableField<V>
{
    fn build(self, cx: &mut Cx<'_, Bevy, EditorTheme>) -> Entity {
        let node = self.inner.build(cx);
        draggable(cx.world, node, self.field, self.label);
        node
    }
}

/// Makes `node` a drag source for `field`, named `label` while it
/// follows the cursor.
fn draggable(
    world: &mut World,
    node: Entity,
    field: Field,
    label: String,
) {
    world
        .entity_mut(node)
        .insert(EntityCursor(SystemCursorIcon::Grab))
        .observe(
            move |start: On<Pointer<DragStart>>,
                  theme: Res<Theme<EditorTheme>>,
                  scale: Res<UiScale>,
                  mut dragged: ResMut<DraggedField>,
                  mut commands: Commands| {
                if start.button != PointerButton::Primary {
                    return;
                }
                let at = start.logical(&scale);

                dragged.field = Some(field.clone());
                dragged.ghost = Some(
                    commands
                        .spawn(ghost(at, label.clone(), &theme.0))
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
                follow(&mut node, drag.logical(&scale));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::Field;
    use crate::tests::{self, Probe};

    /// What the host's checks answer, in the world.
    #[derive(Resource, Default)]
    struct Host {
        has_action: bool,
    }

    fn animatable(_: &World, field: &Field) -> bool {
        field.path() == "level"
    }

    fn has_action(world: &World, _: &Field) -> bool {
        world.resource::<Host>().has_action
    }

    fn named(app: &mut App, probe: Entity, path: &str) -> Entity {
        app.init_resource::<Host>();
        app.world_mut().resource_mut::<FieldAnimatable>().0 =
            Some(animatable);
        app.world_mut().resource_mut::<FieldHasAction>().0 =
            Some(has_action);
        let field = Field::of::<Probe>(probe).child(path);
        tests::show(app, field_name(Some(field), path))
    }

    fn ink(app: &App, node: Entity) -> Color {
        app.world().get::<TextColor>(node).unwrap().0
    }

    #[test]
    fn a_has_action_field_turns_its_label_accent() {
        let (mut app, probe) = tests::probe_app();
        let node = named(&mut app, probe, "level");
        let theme = EditorTheme::default();
        assert_eq!(ink(&app, node), theme.color.text);

        app.world_mut().resource_mut::<Host>().has_action = true;
        app.update();
        assert_eq!(ink(&app, node), theme.color.accent);
        assert!(app.world().get::<HasAction>(node).is_some());

        app.world_mut().resource_mut::<Host>().has_action = false;
        app.update();
        assert_eq!(ink(&app, node), theme.color.text);
        assert!(app.world().get::<HasAction>(node).is_none());
    }

    #[test]
    fn a_label_keeps_its_own_colour_when_the_action_goes() {
        let (mut app, probe) = tests::probe_app();
        app.init_resource::<Host>();
        app.world_mut().resource_mut::<FieldAnimatable>().0 =
            Some(animatable);
        app.world_mut().resource_mut::<FieldHasAction>().0 =
            Some(has_action);
        let red = EditorTheme::default().palette.red;
        let field = Field::of::<Probe>(probe).child("level");
        let node = tests::show(
            &mut app,
            field_name(Some(field), "X").ink(red),
        );
        assert_eq!(ink(&app, node), red);

        app.world_mut().resource_mut::<Host>().has_action = true;
        app.update();
        assert_ne!(ink(&app, node), red);
        app.world_mut().resource_mut::<Host>().has_action = false;
        app.update();
        assert_eq!(ink(&app, node), red);
    }

    #[test]
    fn only_an_animatable_field_is_a_drag_source() {
        let (mut app, probe) = tests::probe_app();
        let plain = named(&mut app, probe, "name");
        assert!(app.world().get::<EntityCursor>(plain).is_none());

        let source = named(&mut app, probe, "level");
        assert!(app.world().get::<EntityCursor>(source).is_some());
    }

    #[test]
    fn dragging_a_label_carries_its_field_with_a_ghost() {
        let (mut app, probe) = tests::probe_app();
        let node = named(&mut app, probe, "level");

        tests::drag_start(&mut app, node, PointerButton::Primary);
        let dragged = app.world().resource::<DraggedField>();
        assert_eq!(
            dragged.field,
            Some(Field::of::<Probe>(probe).child("level"))
        );
        let ghost = dragged.ghost.expect("a ghost");
        assert!(app.world().get::<Node>(ghost).is_some());

        tests::drag_stop(&mut app, node);
        let dragged = app.world().resource::<DraggedField>();
        assert_eq!(dragged.field, None);
        assert!(app.world().get_entity(ghost).is_err());
    }

    #[test]
    fn another_button_does_not_start_a_drag() {
        let (mut app, probe) = tests::probe_app();
        let node = named(&mut app, probe, "level");

        tests::drag_start(&mut app, node, PointerButton::Secondary);

        assert_eq!(
            app.world().resource::<DraggedField>().field,
            None
        );
    }
}
