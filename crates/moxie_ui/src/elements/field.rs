//! What an inspector row is edited with.

use crate::reactive::FynixBuild;
use bevy::feathers::controls::{
    FeathersNumberInput, FeathersTextInput,
    FeathersTextInputContainer, NumberFormat, NumberInputValue,
    UpdateNumberInput,
};
use bevy::feathers::cursor::{EntityCursor, OverrideCursor};
use bevy::feathers::theme::UiTheme;
use bevy::feathers::tokens;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{
    FocusCause, FocusGained, FocusLost, FocusedInput, InputFocus,
};
use bevy::picking::Pickable;
use bevy::picking::events::{Click, Drag, DragEnd, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::scene::EntityWorldMutSceneExt;
use bevy::text::{EditableText, TextCursorStyle, TextEdit};
use bevy::ui::Checked;
use bevy::ui_widgets::{Checkbox as CheckboxBehavior, ValueChange};
use bevy::window::SystemCursorIcon;
use bevy_fynix::WorldEntityMut;
use fynix::element::element;

use super::patch::*;

/// A box that is ticked or not.
#[element(build = Self::build)]
pub struct CheckBox {
    #[elem(patch = PatchChecked)]
    pub checked: bool,
    #[elem(default = theme.color.fill, patch = PatchBackground)]
    pub fill: Color,
    #[elem(default = theme.color.accent, patch = PatchMark)]
    pub mark: Color,
}

/// The inner square a ticked box shows, on a child of its own: a node
/// draws one background, and the box's own is the field it sits in.
#[derive(Component)]
struct CheckMark;

/// Toggle the `Checked` marker and show or hide the inner square.
pub(super) fn tick(checked: bool, entity: &mut impl WorldEntityMut) {
    if checked {
        entity.insert(Checked);
    } else {
        entity.remove::<Checked>();
    }

    let node = entity.id();
    let world = entity.world_mut();
    let Some(mark) = mark_node(world, node) else {
        return;
    };
    if let Some(mut layout) = world.get_mut::<Node>(mark) {
        layout.display = if checked {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Paint the inner square.
pub(super) fn paint(mark: Color, entity: &mut impl WorldEntityMut) {
    let node = entity.id();
    let world = entity.world_mut();
    let Some(spot) = mark_node(world, node) else {
        return;
    };
    world.entity_mut(spot).insert(BackgroundColor(mark));
}

/// The mark the build hook spawned, found by its marker rather than
/// by position: a box may be given children of its own.
fn mark_node(world: &World, node: Entity) -> Option<Entity> {
    world
        .get::<Children>(node)?
        .iter()
        .find(|&child| world.get::<CheckMark>(child).is_some())
}

impl CheckBox {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        build
            .insert((
                CheckboxBehavior,
                EntityCursor::System(SystemCursorIcon::Pointer),
                Node {
                    width: px(16),
                    height: px(16),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(self.fill),
            ))
            .with_child((
                CheckMark,
                Node {
                    width: px(8),
                    height: px(8),
                    border_radius: BorderRadius::all(px(2)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(self.mark),
            ));

        tick(self.checked, build);
    }
}

field_patch!(PatchChecked, bool, |patch, v| tick(*v, patch));

field_patch!(PatchMark, Color, |patch, v| paint(*v, patch));

/// A number, typed or dragged.
#[element(build = Self::build)]
pub struct NumberField {
    #[elem(patch = PatchNumberFormat)]
    pub format: NumberFormat,
    /// The value shown. A field with focus keeps what is typed there
    /// rather than taking this.
    #[elem(default = NumberInputValue::F32(0.0), patch = PatchNumberValue)]
    pub value: NumberInputValue,
    #[elem(default = px(80), patch = PatchWidth)]
    pub width: Val,
}

/// Feathers builds the input as a scene of its own: a container, the
/// two steppers, and the text in between. Widening the node it wrote,
/// not replacing it - the container carries the row's height, padding
/// and rim, and an input without them has nothing to type into.
pub(super) fn number_scene(
    format: NumberFormat,
    width: Val,
    entity: &mut impl WorldEntityMut,
) {
    // `apply_scene` keeps the subtree a prior call built, and this
    // runs again whenever `format` is patched. Clear it so the stale
    // input is not left as a rootless node.
    entity.entity_mut().despawn_related::<Children>();

    let scene = bsn! {
        @FeathersNumberInput { @number_format: {format} }
    };
    if let Err(err) = entity.entity_mut().apply_scene(scene) {
        error!("failed to build a number field: {err}");
    }
    if let Some(mut layout) = entity.entity_mut().get_mut::<Node>() {
        layout.width = width;
        layout.flex_grow = 0.0;
    }
    style_caret(entity);
    let field = entity.id();
    modes(entity.world_mut(), field);
}

/// Styles the caret of the input under `entity` from the theme.
/// Feathers styles one only when its theme changes, so an input built
/// after that keeps bevy's own default.
fn style_caret(entity: &mut impl WorldEntityMut) {
    let node = entity.id();
    let world = entity.world_mut();
    let Some(input) = TextField::text_input(world, node) else {
        return;
    };
    let theme = world.resource::<UiTheme>();
    let caret = TextCursorStyle {
        color: theme.color(&tokens::TEXT_INPUT_CURSOR),
        selection_color: theme.color(&tokens::TEXT_INPUT_SELECTION),
        unfocused_selection_color: theme
            .color(&tokens::TEXT_INPUT_SELECTION_UNFOCUSED),
        selected_text_color: None,
    };
    world.entity_mut(input).insert(caret);
}

impl NumberField {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        number_scene(self.format, self.width, build);

        // On the field rather than its text, which a format change
        // rebuilds. In drag mode the text ignores the pointer, so all
        // of these land here.
        let field = build.id();
        build
            .observe(
                move |drag: On<Pointer<Drag>>,
                      mut commands: Commands| {
                    if drag.button != PointerButton::Primary {
                        return;
                    }
                    let dx = drag.distance.x;
                    commands.queue(move |world: &mut World| {
                        scrub(world, field, dx);
                    });
                },
            )
            .observe(
                move |_: On<Pointer<DragEnd>>,
                      mut commands: Commands| {
                    commands.queue(move |world: &mut World| {
                        end_scrub(world, field);
                    });
                },
            )
            .observe(
                move |click: On<Pointer<Click>>,
                      mut commands: Commands| {
                    if click.button != PointerButton::Primary {
                        return;
                    }
                    commands.queue(move |world: &mut World| {
                        type_into(world, field);
                    });
                },
            );
    }
}

/// Puts `field` in its two modes. Dragged, it scrubs, and its text
/// ignores the pointer. Typed into, its text has focus and takes the
/// pointer back, to place a caret and select with.
fn modes(world: &mut World, field: Entity) {
    let Some(input) = TextField::text_input(world, field) else {
        return;
    };
    world.entity_mut(field).insert(DRAG_CURSOR);
    world
        .entity_mut(input)
        .insert(Pickable::IGNORE)
        .observe(|gained: On<FocusGained>, mut commands: Commands| {
            commands
                .entity(gained.entity)
                .insert(Pickable::default());
        })
        .observe(|lost: On<FocusLost>, mut commands: Commands| {
            commands.entity(lost.entity).insert(Pickable::IGNORE);
        })
        .observe(
            |key: On<FocusedInput<KeyboardInput>>,
             mut focus: ResMut<InputFocus>| {
                if key.input.key_code == KeyCode::Enter
                    && key.input.state.is_pressed()
                {
                    focus.clear();
                }
            },
        );
}

/// Switches `field` to typing, with its whole value selected to type
/// over. Not at the end of a scrub, and not when it is typed into
/// already.
fn type_into(world: &mut World, field: Entity) {
    if world.get::<Scrub>(field).is_some() {
        return;
    }
    let Some(input) = TextField::text_input(world, field) else {
        return;
    };
    let mut focus = world.resource_mut::<InputFocus>();
    if focus.get() == Some(input) {
        return;
    }
    focus.set(input, FocusCause::Pressed);
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        text.queue_edit(TextEdit::SelectAll);
    }
}

/// What a scrubbed field held when the scrub began.
#[derive(Component)]
struct Scrub(NumberInputValue);

/// How far a press must move sideways before it scrubs rather than
/// clicks, in logical pixels.
const SCRUB_SLOP: f32 = 3.0;
/// How much a float changes per pixel dragged.
const SCRUB_STEP: f64 = 0.01;
/// How many pixels an integer is dragged per unit.
const SCRUB_PIXELS_PER_UNIT: f32 = 4.0;
const DRAG_CURSOR: EntityCursor =
    EntityCursor::System(SystemCursorIcon::EwResize);

/// Scrubs `field` to `dx` pixels past where the drag began.
fn scrub(world: &mut World, field: Entity, dx: f32) {
    let start = match world.get::<Scrub>(field) {
        Some(scrub) => scrub.0,
        None => {
            if dx.abs() < SCRUB_SLOP {
                return;
            }
            let Some(start) = shown(world, field) else {
                return;
            };
            world.entity_mut(field).insert(Scrub(start));
            // Held however far the pointer strays from the field.
            world.resource_mut::<OverrideCursor>().0 =
                Some(DRAG_CURSOR);
            start
        }
    };

    let value = match start {
        NumberInputValue::F32(v) => {
            NumberInputValue::F32(nudge(v as f64, dx) as f32)
        }
        NumberInputValue::F64(v) => {
            NumberInputValue::F64(nudge(v, dx))
        }
        NumberInputValue::I32(v) => NumberInputValue::I32(
            v + (dx / SCRUB_PIXELS_PER_UNIT).round() as i32,
        ),
        NumberInputValue::I64(v) => NumberInputValue::I64(
            v + (dx / SCRUB_PIXELS_PER_UNIT).round() as i64,
        ),
    };
    world.trigger(UpdateNumberInput {
        entity: field,
        value,
    });
    changed(world, field, value, false);
}

/// `start` moved `dx` pixels' worth, kept to the step so it reads
/// cleanly.
fn nudge(start: f64, dx: f32) -> f64 {
    let steps = (dx as f64).round();
    ((start + steps * SCRUB_STEP) / SCRUB_STEP).round() * SCRUB_STEP
}

/// Ends a scrub on `field`, if one is under way, and settles on where
/// it got to.
fn end_scrub(world: &mut World, field: Entity) {
    if world.entity_mut(field).take::<Scrub>().is_none() {
        return;
    }
    let mut cursor = world.resource_mut::<OverrideCursor>();
    if cursor.0 == Some(DRAG_CURSOR) {
        cursor.0 = None;
    }
    if let Some(value) = shown(world, field) {
        changed(world, field, value, true);
    }
}

/// The number `field` shows.
fn shown(world: &World, field: Entity) -> Option<NumberInputValue> {
    let format = *world.get::<NumberFormat>(field)?;
    let input = TextField::text_input(world, field)?;
    let text = world.get::<EditableText>(input)?.value().to_string();
    let text = text.trim();
    Some(match format {
        NumberFormat::F32 => {
            NumberInputValue::F32(text.parse().ok()?)
        }
        NumberFormat::F64 => {
            NumberInputValue::F64(text.parse().ok()?)
        }
        NumberFormat::I32 => {
            NumberInputValue::I32(text.parse().ok()?)
        }
        NumberFormat::I64 => {
            NumberInputValue::I64(text.parse().ok()?)
        }
    })
}

/// Tells whoever listens on `field` that it now holds `value`, the
/// way a typed edit does.
fn changed(
    world: &mut World,
    field: Entity,
    value: NumberInputValue,
    is_final: bool,
) {
    match value {
        NumberInputValue::F32(value) => world.trigger(ValueChange {
            source: field,
            value,
            is_final,
        }),
        NumberInputValue::F64(value) => world.trigger(ValueChange {
            source: field,
            value,
            is_final,
        }),
        NumberInputValue::I32(value) => world.trigger(ValueChange {
            source: field,
            value,
            is_final,
        }),
        NumberInputValue::I64(value) => world.trigger(ValueChange {
            source: field,
            value,
            is_final,
        }),
    }
}

field_patch!(PatchNumberFormat, NumberFormat, |patch, v| {
    let width = patch
        .entity_mut()
        .get::<Node>()
        .map(|node| node.width)
        .unwrap_or(px(80));
    number_scene(*v, width, patch);
});

field_patch!(PatchNumberValue, NumberInputValue, |patch, v| {
    let node = patch.id();
    let Some(input) = TextField::text_input(patch.world, node) else {
        return;
    };
    // The focused field keeps what the user typed.
    if patch.world.resource::<InputFocus>().get() == Some(input) {
        return;
    }
    let shown = v.to_string();
    if let Some(mut text) = patch.world.get_mut::<EditableText>(input)
        && text.value().to_string() != shown
    {
        text.editor_mut().set_text(&shown);
    }
});

/// A single-line string, edited in place.
#[element(build = Self::build)]
pub struct TextField {
    #[elem(patch = PatchTextValue)]
    pub value: String,
    #[elem(default = px(110), patch = PatchWidth)]
    pub width: Val,
}

impl TextField {
    /// The child entity actually holding [`EditableText`], found by
    /// marker rather than position - the container may end up with
    /// other children later (an icon, say), and nothing here should
    /// depend on which one comes first. What a caller reaching past
    /// this element's own fields (to wire an observer directly, say)
    /// needs too.
    pub fn text_input(world: &World, node: Entity) -> Option<Entity> {
        world
            .get::<Children>(node)?
            .iter()
            .find(|&child| world.get::<EditableText>(child).is_some())
    }
}

/// Feathers wants its own child entity for the editable text - see
/// the type's own docs - so this node is the container, and the
/// widget beneath it what actually holds [`EditableText`].
fn text_scene(
    width: Val,
    value: &str,
    entity: &mut impl WorldEntityMut,
) {
    let scene = bsn! {
        @FeathersTextInputContainer
        Children [
            ( @FeathersTextInput )
        ]
    };
    if let Err(err) = entity.entity_mut().apply_scene(scene) {
        error!("failed to build a text field: {err}");
    }

    if let Some(mut layout) = entity.entity_mut().get_mut::<Node>() {
        layout.width = width;
        layout.flex_grow = 0.0;

        // `FeathersTextInputContainer` reserves its left inset as a
        // colorless 3px *border* rather than padding (room for a
        // leading icon we never add), which leaves the background
        // unpainted there and the left corners looking square next to
        // the fully rounded right ones. Folding it into padding
        // instead paints the background - and so the radius - all the
        // way around.
        layout.border = UiRect::ZERO;
        layout.padding = UiRect::horizontal(px(3.0));
    }

    set_text(value, entity);
    style_caret(entity);
}

/// Write `value` into the child [`EditableText`].
pub(super) fn set_text(
    value: &str,
    entity: &mut impl WorldEntityMut,
) {
    let node = entity.id();
    let world = entity.world_mut();
    let Some(input) = TextField::text_input(world, node) else {
        return;
    };
    if let Some(mut text) = world.get_mut::<EditableText>(input) {
        text.editor_mut().set_text(value);
    }
}

impl TextField {
    fn build(&self, build: &mut FynixBuild<'_, Self>) {
        text_scene(self.width, &self.value, build);
    }
}

field_patch!(PatchTextValue, String, |patch, v| set_text(v, patch));
