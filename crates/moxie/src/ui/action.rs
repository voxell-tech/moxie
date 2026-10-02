//! Inspects whatever the timeline has selected: an action's own
//! properties, or a block's.
//!
//! The reflect inspector cannot reach these: it addresses one
//! component of one entity, and an action is scene data. This edits
//! the [`EditorScene`] directly, by the path the
//! timeline selected the node with.

use core::time::Duration;

use bevy::asset::uuid::Uuid;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps as _, column, label, row, scroll, segmented,
};
use bevy_fynix::{AnyView, Bevy, ViewExt as _, keyed};
use bevy_motiongfx::scene::backend::{AnimEase, AnimInterp, Backend};
use motiongfx_scene::block::{Block, Combinator, Node};
use motiongfx_scene::refs::FieldRef;
use moxie_ui::elements::display_name;
use moxie_ui::gaps::{anchored, changing_under};
use moxie_ui::inspector::{
    Binding, Source, field_row, inspect_value, reflect_changed,
};
use moxie_ui::theme::EditorTheme;

use crate::{EditorScene, EditorSettings, SelectedAction, subject};

/// The stagger a block gets when the Type picker switches it to
/// `Flow`.
const DEFAULT_STAGGER: Duration = Duration::from_millis(150);

/// The action panel.
///
/// Built again only when the shape of the selection changes: each
/// input binds its own value, so typing into one never rebuilds the
/// panel out from under it.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let pad = cx.theme().space.xl;
        cx.build(anchored::<EditorTheme, _>(move |anchor| {
            keyed::<EditorTheme, Shape>(
                changing_under(anchor, shape),
                |shape| contents(shape.clone()),
            )
            .within(
                scroll(())
                    .width(percent(100.0))
                    .grow(1.0)
                    .gap(8.0)
                    .padding(UiRect::all(px(pad))),
            )
        }))
    })
}

/// One property of the selected node that an input writes back.
///
/// Named: an input re-reads and rewrites it long after the panel was
/// built.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Edit {
    /// How long the action runs for.
    Duration,
    /// The node's own offset from where its parent starts it.
    Delay,
    /// How far apart a `Flow` block staggers its children.
    Stagger,
    /// The curve the action follows.
    Ease,
    /// How its values are blended.
    Interp,
    /// The node's own name, block or action alike.
    Name,
}

/// Everything a rebuild depends on. The numbers are deliberately
/// absent: they move without rebuilding anything.
#[derive(Clone, PartialEq)]
struct Shape {
    path: Option<Vec<usize>>,
    /// Empty when the path no longer lands on a node.
    kind: &'static str,
    /// Set only for an action: a block has none to show.
    subject: Option<subject::Caption>,
    /// Set only for a block: which of Chain/All/Flow it is, for the
    /// Type row's picker. An action has none to show.
    combinator: Option<&'static str>,
    rows: Vec<(String, String)>,
    edits: Vec<(String, Edit)>,
    /// The action's target value. Which widget draws it is the
    /// registry's business.
    value: Option<Pooled>,
}

/// The action's target value, wherever the pool keeps it.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Pooled(Uuid);

/// [`Edit::Interp`]'s own reflected type, standing in for
/// `Option<AnimInterp>`.
#[derive(Reflect, Clone, Copy, PartialEq, Debug)]
enum InterpChoice {
    /// `Option::None`: jumps to the target at the end instead of
    /// interpolating toward it.
    Step,
    Linear,
}

impl From<Option<AnimInterp>> for InterpChoice {
    fn from(interp: Option<AnimInterp>) -> Self {
        match interp {
            None => Self::Step,
            Some(AnimInterp::Linear) => Self::Linear,
        }
    }
}

impl From<InterpChoice> for Option<AnimInterp> {
    fn from(choice: InterpChoice) -> Self {
        match choice {
            InterpChoice::Step => None,
            InterpChoice::Linear => Some(AnimInterp::Linear),
        }
    }
}

/// One property of the selected node, as somewhere a widget can read
/// and write. Which widget that is follows from the type it hands
/// back, so this never names one.
#[derive(Clone)]
struct Property {
    path: Vec<usize>,
    edit: Edit,
}

/// The current selection's shape.
fn shape(world: &World) -> Shape {
    let path = world
        .get_resource::<SelectedAction>()
        .and_then(|selected| selected.0.clone());

    path.as_ref()
        .and_then(|path| summarize(world, path))
        .unwrap_or(Shape {
            path,
            kind: "",
            subject: None,
            combinator: None,
            rows: Vec::new(),
            edits: Vec::new(),
            value: None,
        })
}

/// A dimmed note, for a panel with nothing to show.
fn note(text: &str) -> AnyView<Bevy, EditorTheme> {
    label(text.to_string()).tone(Tone::Dim).boxed()
}

/// A row of `value` under a dimmed `name`.
fn labelled(
    name: impl Into<String>,
    value: AnyView<Bevy, EditorTheme>,
) -> AnyView<Bevy, EditorTheme> {
    field_row(
        Some(label(name.into()).tone(Tone::Dim).boxed()),
        value,
        0,
    )
    .boxed()
}

/// The panel's rows for `shape`.
fn contents(shape: Shape) -> AnyView<Bevy, EditorTheme> {
    let Some(path) = shape.path else {
        return note("Nothing selected");
    };
    if shape.kind.is_empty() {
        return note("Selection is no longer in the scene");
    }

    let mut rows = vec![heading(path.clone())];
    if let Some(subject) = shape.subject {
        rows.push(labelled("Subject", caption(subject)));
    }
    if let Some(combinator) = shape.combinator {
        rows.push(labelled(
            "Type",
            type_picker(combinator, path.clone()),
        ));
    }
    for (name, value) in shape.rows {
        rows.push(labelled(name, label(value).wrap(false).boxed()));
    }
    for (name, edit) in shape.edits {
        let source = Property {
            path: path.clone(),
            edit,
        };
        rows.push(labelled(
            name,
            inspect_value(Binding::new(source)),
        ));
    }
    if let Some(pooled) = shape.value {
        rows.push(labelled(
            "Value",
            inspect_value(Binding::new(pooled)),
        ));
    }

    column(rows).width(percent(100.0)).gap(8.0).boxed()
}

/// A subject's name, then its id head muted.
fn caption(subject: subject::Caption) -> AnyView<Bevy, EditorTheme> {
    let name =
        subject.name.map(|name| label(name).wrap(false).boxed());
    let head = label(format!("#{}", subject.head))
        .tone(Tone::Dim)
        .wrap(false)
        .boxed();
    row(name.into_iter().chain([head]).collect::<Vec<_>>())
        .align(AlignItems::Center)
        .gap(4.0)
        .boxed()
}

/// The Chain/All/Flow picker of the block at `path`.
fn type_picker(
    combinator: &'static str,
    path: Vec<usize>,
) -> AnyView<Bevy, EditorTheme> {
    let selected = match combinator {
        "Chain" => 0,
        "All" => 1,
        _ => 2,
    };
    segmented(
        ["Chain", "All", "Flow"],
        selected,
        move |world, index| set_combinator(world, &path, index),
    )
    .boxed()
}

/// The panel's heading: the node's name, as an editable text field.
fn heading(path: Vec<usize>) -> AnyView<Bevy, EditorTheme> {
    inspect_value(Binding::new(Property {
        path,
        edit: Edit::Name,
    }))
}

/// The selected node, as what to show and what can be changed.
///
/// An empty `path` names the tree's own root - a block like any
/// other, just never wrapped in a [`Node`] of its own, so it has no
/// delay to show or edit.
fn summarize(world: &World, path: &[usize]) -> Option<Shape> {
    let scene = world.get_resource::<EditorScene>()?.scene();

    if path.is_empty() {
        return Some(block_shape(
            Vec::new(),
            &scene.0.animation,
            None,
        ));
    }

    let node = node_at(&scene.0.animation, path)?;
    let (delay, action) = match node {
        Node::Block { block, delay } => {
            return Some(block_shape(path.to_vec(), block, *delay));
        }
        Node::Draft { delay, .. } => {
            return Some(draft_shape(path.to_vec(), *delay));
        }
        Node::Action { action, delay } => (delay, action),
    };

    let mut edits = vec![("Duration".to_string(), Edit::Duration)];
    if delay.is_some() {
        edits.push(("Delay".to_string(), Edit::Delay));
    }
    edits.push(("Ease".to_string(), Edit::Ease));
    edits.push(("Interpolation".to_string(), Edit::Interp));

    Some(Shape {
        path: Some(path.to_vec()),
        kind: "Action",
        subject: Some(subject::Caption::of(world, action.subject)),
        combinator: None,
        value: Some(Pooled(action.value)),
        rows: vec![
            ("Field".into(), field_name(world, &action.field)),
            ("Operation".into(), format!("{:?}", action.op)),
        ],
        edits,
    })
}

/// A block's own row of info: its combinator, plus whatever of its
/// timing the [`Node`] wrapping it (if any) lets through.
fn block_shape(
    path: Vec<usize>,
    block: &Block<Backend>,
    delay: Option<Duration>,
) -> Shape {
    let mut edits = Vec::new();
    if matches!(block.combinator, Combinator::Flow(_)) {
        edits.push(("Stagger".to_string(), Edit::Stagger));
    }
    if delay.is_some() {
        edits.push(("Delay".to_string(), Edit::Delay));
    }

    Shape {
        path: Some(path),
        kind: "Block",
        subject: None,
        combinator: Some(combinator_name(&block.combinator)),
        value: None,
        rows: Vec::new(),
        edits,
    }
}

/// A draft's own row of info - a subject and field yet to be picked,
/// so its Subject/Field rows are placeholder text for now rather than
/// the pickers that will land alongside dragging a field onto the
/// timeline.
fn draft_shape(path: Vec<usize>, delay: Option<Duration>) -> Shape {
    let mut edits = vec![("Duration".to_string(), Edit::Duration)];
    if delay.is_some() {
        edits.push(("Delay".to_string(), Edit::Delay));
    }

    Shape {
        path: Some(path),
        kind: "Draft",
        subject: None,
        combinator: None,
        value: None,
        rows: vec![
            ("Subject".into(), "Unassigned".into()),
            ("Field".into(), "Unassigned".into()),
        ],
        edits,
    }
}

/// The node `path` names.
pub(super) fn node_at<'a>(
    root: &'a Block<Backend>,
    path: &[usize],
) -> Option<&'a Node<Backend>> {
    let (&first, rest) = path.split_first()?;

    let mut node = root.children.get(first)?;
    for &index in rest {
        let Node::Block { block, .. } = node else {
            return None;
        };
        node = block.children.get(index)?;
    }
    Some(node)
}

/// The same walk, to change what it lands on.
pub(super) fn node_at_mut<'a>(
    root: &'a mut Block<Backend>,
    path: &[usize],
) -> Option<&'a mut Node<Backend>> {
    let (&first, rest) = path.split_first()?;

    let mut node = root.children.get_mut(first)?;
    for &index in rest {
        let Node::Block { block, .. } = node else {
            return None;
        };
        node = block.children.get_mut(index)?;
    }
    Some(node)
}

/// The pooled value, whatever column it lives in.
///
/// The type comes back with the value, so nothing here has to name
/// which of them it is.
impl Source for Pooled {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        let values =
            &world.get_resource::<EditorScene>()?.scene().0.values;

        values
            .f32
            .get(&self.0)
            .map(|value| Box::new(*value) as Box<dyn PartialReflect>)
            .or_else(|| {
                values.vec3.get(&self.0).map(|value| {
                    Box::new(*value) as Box<dyn PartialReflect>
                })
            })
            .or_else(|| {
                values.quat.get(&self.0).map(|value| {
                    Box::new(*value) as Box<dyn PartialReflect>
                })
            })
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        let Some(mut editor) =
            world.get_resource_mut::<EditorScene>()
        else {
            return;
        };
        let values = &mut editor.edit().values;

        if let Some(slot) = values.f32.get_mut(&self.0) {
            let _ = slot.try_apply(value);
        } else if let Some(slot) = values.vec3.get_mut(&self.0) {
            let _ = slot.try_apply(value);
        } else if let Some(slot) = values.quat.get_mut(&self.0) {
            let _ = slot.try_apply(value);
        }
    }

    fn changed(
        &self,
    ) -> Box<dyn FnMut(&World) -> bool + Send + Sync> {
        let pooled = *self;
        Box::new(reflect_changed(move |world| pooled.get(world)))
    }

    fn boxed(&self) -> Box<dyn Source> {
        Box::new(*self)
    }
}

/// One of the node's own properties, reflected.
impl Source for Property {
    fn get(&self, world: &World) -> Option<Box<dyn PartialReflect>> {
        let scene = world.get_resource::<EditorScene>()?.scene();

        // The root has no `Node` of its own, so no `Ease`, `Interp`,
        // `Duration` or `Delay` - only ever reached for its own
        // `Stagger` and `Name`.
        if self.path.is_empty() {
            return match self.edit {
                Edit::Name => Some(Box::new(
                    scene
                        .0
                        .animation
                        .name
                        .clone()
                        .unwrap_or_default(),
                )),
                _ => Some(Box::new(stagger_seconds(
                    &scene.0.animation,
                )?)),
            };
        }
        let node = node_at(&scene.0.animation, &self.path)?;

        match (self.edit, node) {
            // Ease's `None` is the default curve, linear, so the
            // picker shows that instead of a fourth "unset" state -
            // unlike interp, `None` here is not a distinct behavior.
            (Edit::Ease, Node::Action { action, .. }) => Some(
                Box::new(action.ease.unwrap_or(AnimEase::Linear)),
            ),
            (Edit::Interp, Node::Action { action, .. }) => {
                Some(Box::new(InterpChoice::from(action.interp)))
            }
            // Unset shows as the widget's own empty state.
            (Edit::Name, Node::Block { block, .. }) => {
                Some(Box::new(block.name.clone().unwrap_or_default()))
            }
            (Edit::Name, Node::Action { action, .. }) => Some(
                Box::new(action.name.clone().unwrap_or_default()),
            ),
            (Edit::Name, Node::Draft { name, .. }) => {
                Some(Box::new(name.clone().unwrap_or_default()))
            }
            _ => Some(Box::new(seconds(node, self.edit)?)),
        }
    }

    fn set(&self, world: &mut World, value: &dyn PartialReflect) {
        let min_duration = world
            .get_resource::<EditorSettings>()
            .map(EditorSettings::min_duration)
            .unwrap_or_default();
        let Some(mut editor) =
            world.get_resource_mut::<EditorScene>()
        else {
            return;
        };
        let scene = editor.edit();

        if self.path.is_empty() {
            match self.edit {
                Edit::Name => {
                    if let Some(value) = String::from_reflect(value) {
                        scene.0.animation.name = named(value);
                    }
                }
                _ => {
                    if let Some(value) = f32::from_reflect(value) {
                        scene.0.animation.combinator =
                            Combinator::Flow(clamp_seconds(value));
                    }
                }
            }
            return;
        }
        let Some(node) =
            node_at_mut(&mut scene.0.animation, &self.path)
        else {
            return;
        };

        match (self.edit, node) {
            (Edit::Ease, Node::Action { action, .. }) => {
                action.ease = AnimEase::from_reflect(value);
            }
            (Edit::Interp, Node::Action { action, .. }) => {
                if let Some(choice) =
                    InterpChoice::from_reflect(value)
                {
                    action.interp = choice.into();
                }
            }
            (Edit::Name, Node::Block { block, .. }) => {
                if let Some(value) = String::from_reflect(value) {
                    block.name = named(value);
                }
            }
            (Edit::Name, Node::Action { action, .. }) => {
                if let Some(value) = String::from_reflect(value) {
                    action.name = named(value);
                }
            }
            (Edit::Name, Node::Draft { name, .. }) => {
                if let Some(value) = String::from_reflect(value) {
                    *name = named(value);
                }
            }
            (edit, node) => {
                if let Some(value) = f32::from_reflect(value) {
                    set_seconds(node, edit, value, min_duration);
                }
            }
        }
    }

    fn changed(
        &self,
    ) -> Box<dyn FnMut(&World) -> bool + Send + Sync> {
        let property = self.clone();
        Box::new(reflect_changed(move |world| property.get(world)))
    }

    fn boxed(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }
}

/// What `edit` reads as, for the properties measured in seconds.
fn seconds(node: &Node<Backend>, edit: Edit) -> Option<f32> {
    match (edit, node) {
        (Edit::Delay, Node::Block { delay, .. })
        | (Edit::Delay, Node::Action { delay, .. })
        | (Edit::Delay, Node::Draft { delay, .. }) => {
            Some(delay.unwrap_or_default().as_secs_f32())
        }
        (Edit::Stagger, Node::Block { block, .. }) => {
            stagger_seconds(block)
        }
        (Edit::Duration, Node::Action { action, .. }) => {
            Some(action.duration.as_secs_f32())
        }
        (Edit::Duration, Node::Draft { duration, .. }) => {
            Some(duration.as_secs_f32())
        }
        _ => None,
    }
}

/// A `Flow` block's own stagger, in seconds - `None` for any other
/// combinator.
fn stagger_seconds(block: &Block<Backend>) -> Option<f32> {
    match block.combinator {
        Combinator::Flow(delay) => Some(delay.as_secs_f32()),
        _ => None,
    }
}

/// Never negative: nothing here runs backwards.
fn clamp_seconds(value: f32) -> Duration {
    Duration::from_secs_f32(value.max(0.0))
}

/// Blank input clears a name back to `None`.
fn named(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Writes one of the properties measured in seconds.
fn set_seconds(
    node: &mut Node<Backend>,
    edit: Edit,
    value: f32,
    min_duration: Duration,
) {
    let seconds = clamp_seconds(value);

    match (edit, node) {
        (Edit::Delay, Node::Block { delay, .. })
        | (Edit::Delay, Node::Action { delay, .. })
        | (Edit::Delay, Node::Draft { delay, .. }) => {
            *delay = Some(seconds);
        }
        (Edit::Stagger, Node::Block { block, .. }) => {
            block.combinator = Combinator::Flow(seconds);
        }
        (Edit::Duration, Node::Action { action, .. }) => {
            action.duration = seconds.max(min_duration);
        }
        (Edit::Duration, Node::Draft { duration, .. }) => {
            *duration = seconds.max(min_duration);
        }
        _ => {}
    }
}

/// The type's own display name, the same [`ReflectInspectable`]-aware
/// one the entity inspector shows it by, followed by the path to the
/// field this action drives - `Transform::translation::x`. Falls back
/// to the type path's own last segment when the registry has never
/// heard of it.
///
/// [`ReflectInspectable`]: moxie_ui::inspector::ReflectInspectable
fn field_name(world: &World, field: &FieldRef) -> String {
    let type_name = field.type_name().to_string();
    let registry = world.resource::<AppTypeRegistry>().read();

    let name = registry
        .get_with_type_path(&type_name)
        .map(display_name)
        .unwrap_or_else(|| {
            type_name
                .rsplit("::")
                .next()
                .unwrap_or(&type_name)
                .to_string()
                .into()
        });

    format!("{name}{}", field.path())
}

/// Writes the block at `path` (the tree's own root, if empty) onto
/// whichever of Chain/All/Flow the Type picker's `index` names.
/// Switching into Flow seeds a default stagger; switching out of it
/// drops whatever stagger it had.
fn set_combinator(world: &mut World, path: &[usize], index: usize) {
    let Some(mut editor) = world.get_resource_mut::<EditorScene>()
    else {
        return;
    };
    let scene = editor.edit();

    let combinator = match index {
        0 => Combinator::Chain,
        1 => Combinator::All,
        _ => Combinator::Flow(DEFAULT_STAGGER),
    };

    if path.is_empty() {
        scene.0.animation.combinator = combinator;
        return;
    }
    if let Some(Node::Block { block, .. }) =
        node_at_mut(&mut scene.0.animation, path)
    {
        block.combinator = combinator;
    }
}

fn combinator_name(combinator: &Combinator) -> &'static str {
    match combinator {
        Combinator::Chain => "Chain",
        Combinator::All => "All",
        // Its stagger is editable, so the row above only names it.
        Combinator::Flow(_) => "Flow",
    }
}

#[cfg(test)]
mod tests {
    use bevy_motiongfx::scene::asset::MotionGfxScene;
    use bevy_motiongfx::scene::backend::AnimOp;
    use bevy_motiongfx::scene::id::{EntityUid, SceneUid};
    use bevy_motiongfx::scene::value_pool::ValuePool;
    use motiongfx_scene::block::ActionCmd;
    use motiongfx_scene::scene::{Scene, Stage};

    use super::*;
    use crate::tests::harness::{Editor, SETTLE};

    const TRANSFORM: &str =
        "bevy_transform::components::transform::Transform";

    fn scene(
        children: Vec<Node<Backend>>,
        values: ValuePool,
    ) -> MotionGfxScene {
        MotionGfxScene(Scene {
            stage: Stage {
                subjects: Vec::new(),
            },
            animation: Block::chain(children),
            values,
        })
    }

    fn draft(delay: Option<Duration>) -> Node<Backend> {
        Node::Draft {
            delay,
            duration: Duration::from_secs(1),
            name: None,
        }
    }

    fn action(subject: SceneUid, value: Uuid) -> ActionCmd<Backend> {
        ActionCmd {
            subject,
            field: FieldRef::new(TRANSFORM, "::translation::x"),
            op: AnimOp::To,
            value,
            duration: Duration::from_secs(2),
            ease: None,
            interp: None,
            name: None,
        }
    }

    fn pooled(value: f32) -> (ValuePool, Uuid) {
        let mut values = ValuePool::default();
        let id = Uuid::new_v4();
        values.f32.insert(id, value);
        (values, id)
    }

    /// A world with just what the panel's reads need, holding
    /// `scene`.
    fn world_of(scene: MotionGfxScene) -> World {
        let mut world = World::new();
        world.init_resource::<AppTypeRegistry>();
        world.insert_resource(EditorScene::new(scene));
        world
    }

    fn property(path: &[usize], edit: Edit) -> Property {
        Property {
            path: path.to_vec(),
            edit,
        }
    }

    fn read<T: FromReflect>(
        source: &impl Source,
        world: &World,
    ) -> T {
        T::from_reflect(&*source.get(world).expect("readable"))
            .expect("of that type")
    }

    fn node(world: &World, path: &[usize]) -> Node<Backend> {
        let scene = world.resource::<EditorScene>().scene();
        node_at(&scene.0.animation, path).expect("there").clone()
    }

    fn action_world() -> (World, Uuid) {
        let (values, id) = pooled(1.5);
        let subject = SceneUid::Entity(EntityUid::new());
        let world = world_of(scene(
            vec![
                Node::Action {
                    delay: Some(Duration::from_secs(1)),
                    action: action(subject, id),
                },
                Node::block(Block {
                    combinator: Combinator::Flow(
                        Duration::from_millis(500),
                    ),
                    ..Block::chain(vec![draft(None)])
                }),
            ],
            values,
        ));
        (world, id)
    }

    #[test]
    fn node_at_walks_down_through_blocks() {
        let inner = Block::chain(vec![draft(None), draft(None)]);
        let root =
            Block::chain(vec![draft(None), Node::block(inner)]);

        assert!(matches!(
            node_at(&root, &[0]),
            Some(Node::Draft { .. })
        ));
        assert!(matches!(
            node_at(&root, &[1, 1]),
            Some(Node::Draft { .. })
        ));
        assert!(node_at(&root, &[]).is_none(), "the root is no node");
        assert!(node_at(&root, &[1, 2]).is_none());
        assert!(
            node_at(&root, &[0, 0]).is_none(),
            "a draft has none"
        );
    }

    #[test]
    fn node_at_mut_lands_on_the_same_node() {
        let mut root =
            Block::chain(vec![Node::block(Block::chain(vec![
                draft(None),
            ]))]);
        let Some(Node::Draft { delay, .. }) =
            node_at_mut(&mut root, &[0, 0])
        else {
            panic!("a draft is at [0, 0]");
        };
        *delay = Some(Duration::from_secs(3));
        assert!(matches!(
            node_at(&root, &[0, 0]),
            Some(Node::Draft { delay: Some(_), .. })
        ));
    }

    #[test]
    fn an_action_summarizes_with_its_own_rows_and_edits() {
        let (world, id) = action_world();
        let shape = summarize(&world, &[0]).expect("an action");

        assert_eq!(shape.kind, "Action");
        assert_eq!(shape.value, Some(Pooled(id)));
        assert!(shape.subject.is_some());
        assert_eq!(shape.combinator, None);
        assert_eq!(
            shape.rows,
            [
                (
                    "Field".to_string(),
                    "Transform::translation::x".to_string()
                ),
                ("Operation".to_string(), "To".to_string()),
            ]
        );
        let edits = shape
            .edits
            .iter()
            .map(|(_, edit)| *edit)
            .collect::<Vec<_>>();
        assert_eq!(
            edits,
            [Edit::Duration, Edit::Delay, Edit::Ease, Edit::Interp]
        );
    }

    #[test]
    fn an_action_without_a_delay_shows_no_delay_row() {
        let (values, id) = pooled(0.0);
        let subject = SceneUid::Entity(EntityUid::new());
        let world = world_of(scene(
            vec![Node::action(action(subject, id))],
            values,
        ));
        let shape = summarize(&world, &[0]).expect("an action");
        assert!(
            shape.edits.iter().all(|(_, edit)| *edit != Edit::Delay)
        );
    }

    #[test]
    fn a_block_summarizes_by_its_combinator() {
        let (world, _) = action_world();
        let flow = summarize(&world, &[1]).expect("a block");
        assert_eq!(flow.kind, "Block");
        assert_eq!(flow.combinator, Some("Flow"));
        assert_eq!(flow.edits[0].1, Edit::Stagger);

        let root = summarize(&world, &[]).expect("the root");
        assert_eq!(root.combinator, Some("Chain"));
        assert!(root.edits.is_empty(), "no stagger, no delay");
    }

    #[test]
    fn a_draft_summarizes_with_placeholder_rows() {
        let world = world_of(scene(
            vec![draft(Some(Duration::ZERO))],
            ValuePool::default(),
        ));
        let shape = summarize(&world, &[0]).expect("a draft");
        assert_eq!(shape.kind, "Draft");
        assert_eq!(shape.value, None);
        assert_eq!(shape.rows[0].1, "Unassigned");
        assert_eq!(shape.rows[1].1, "Unassigned");
        assert_eq!(shape.edits.len(), 2, "duration and delay");
    }

    #[test]
    fn a_path_off_the_tree_summarizes_to_nothing() {
        let (world, _) = action_world();
        assert!(summarize(&world, &[7]).is_none());
        assert!(summarize(&world, &[1, 1]).is_none());
        let shape = shape(&world);
        assert_eq!(shape.path, None, "nothing is selected");
    }

    #[test]
    fn the_shape_follows_the_selection_not_the_numbers() {
        let (mut world, _) = action_world();
        world.insert_resource(SelectedAction(Some(vec![0])));
        let before = shape(&world);

        let duration = property(&[0], Edit::Duration);
        duration.set(&mut world, &9.0f32);
        assert!(shape(&world) == before, "a number is no rebuild");

        world.insert_resource(SelectedAction(Some(vec![1])));
        assert!(shape(&world) != before);

        world.insert_resource(SelectedAction(Some(vec![7])));
        assert_eq!(shape(&world).kind, "");
        assert_eq!(shape(&world).path, Some(vec![7]));
    }

    #[test]
    fn seconds_read_back_what_they_wrote() {
        let (mut world, _) = action_world();
        let duration = property(&[0], Edit::Duration);
        assert_eq!(read::<f32>(&duration, &world), 2.0);

        duration.set(&mut world, &3.5f32);
        assert_eq!(read::<f32>(&duration, &world), 3.5);

        let delay = property(&[0], Edit::Delay);
        delay.set(&mut world, &0.25f32);
        assert_eq!(read::<f32>(&delay, &world), 0.25);

        let stagger = property(&[1], Edit::Stagger);
        assert_eq!(read::<f32>(&stagger, &world), 0.5);
        stagger.set(&mut world, &1.0f32);
        assert_eq!(read::<f32>(&stagger, &world), 1.0);
    }

    #[test]
    fn a_duration_never_falls_below_the_minimum() {
        let (mut world, _) = action_world();
        world.init_resource::<EditorSettings>();
        let min = world.resource::<EditorSettings>().min_duration();

        let duration = property(&[0], Edit::Duration);
        duration.set(&mut world, &0.0f32);
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.duration, min);

        duration.set(&mut world, &-4.0f32);
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.duration, min, "never backwards");
    }

    #[test]
    fn a_delay_clamps_at_zero() {
        let (mut world, _) = action_world();
        let delay = property(&[0], Edit::Delay);
        delay.set(&mut world, &-1.0f32);
        assert_eq!(read::<f32>(&delay, &world), 0.0);
    }

    #[test]
    fn the_root_edits_its_own_name_and_stagger() {
        let (mut world, _) = action_world();
        let name = property(&[], Edit::Name);
        assert_eq!(read::<String>(&name, &world), "");

        name.set(&mut world, &"  Intro  ".to_string());
        assert_eq!(read::<String>(&name, &world), "Intro");

        // The root is a chain, so there is no stagger to read.
        let stagger = property(&[], Edit::Stagger);
        assert!(stagger.get(&world).is_none());
        stagger.set(&mut world, &0.2f32);
        assert_eq!(read::<f32>(&stagger, &world), 0.2);
    }

    #[test]
    fn a_blank_name_clears_it() {
        let (mut world, _) = action_world();
        let name = property(&[0], Edit::Name);
        name.set(&mut world, &"Move".to_string());
        assert_eq!(read::<String>(&name, &world), "Move");

        name.set(&mut world, &"   ".to_string());
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.name, None);

        // A draft and a block name themselves the same way.
        for path in [&[1][..], &[1, 0][..]] {
            let name = property(path, Edit::Name);
            name.set(&mut world, &"Part".to_string());
            assert_eq!(read::<String>(&name, &world), "Part");
        }
    }

    #[test]
    fn ease_shows_linear_while_unset() {
        let (mut world, _) = action_world();
        let ease = property(&[0], Edit::Ease);
        assert_eq!(read::<AnimEase>(&ease, &world), AnimEase::Linear);

        ease.set(&mut world, &AnimEase::CubicEaseInOut);
        assert_eq!(
            read::<AnimEase>(&ease, &world),
            AnimEase::CubicEaseInOut
        );
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.ease, Some(AnimEase::CubicEaseInOut));
    }

    #[test]
    fn interp_is_step_while_unset() {
        let (mut world, _) = action_world();
        let interp = property(&[0], Edit::Interp);
        assert_eq!(
            read::<InterpChoice>(&interp, &world),
            InterpChoice::Step
        );

        interp.set(&mut world, &InterpChoice::Linear);
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.interp, Some(AnimInterp::Linear));

        interp.set(&mut world, &InterpChoice::Step);
        let Node::Action { action, .. } = node(&world, &[0]) else {
            panic!("an action");
        };
        assert_eq!(action.interp, None);
    }

    #[test]
    fn a_property_the_node_lacks_reads_as_nothing() {
        let (world, _) = action_world();
        assert!(property(&[1], Edit::Ease).get(&world).is_none());
        assert!(property(&[0], Edit::Stagger).get(&world).is_none());
        assert!(property(&[9], Edit::Delay).get(&world).is_none());
    }

    #[test]
    fn the_pooled_value_reads_and_writes_its_column() {
        let (mut world, id) = action_world();
        let value = Pooled(id);
        assert_eq!(read::<f32>(&value, &world), 1.5);

        value.set(&mut world, &4.0f32);
        assert_eq!(read::<f32>(&value, &world), 4.0);

        let gone = Pooled(Uuid::new_v4());
        assert!(gone.get(&world).is_none());
    }

    #[test]
    fn the_pooled_value_finds_vectors_and_quaternions() {
        let mut values = ValuePool::default();
        let vec3 = Uuid::new_v4();
        let quat = Uuid::new_v4();
        values.vec3.insert(vec3, Vec3::new(1.0, 2.0, 3.0));
        values.quat.insert(quat, Quat::IDENTITY);
        let mut world = world_of(scene(Vec::new(), values));

        assert_eq!(
            read::<Vec3>(&Pooled(vec3), &world),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            read::<Quat>(&Pooled(quat), &world),
            Quat::IDENTITY
        );

        Pooled(vec3).set(&mut world, &Vec3::ONE);
        assert_eq!(read::<Vec3>(&Pooled(vec3), &world), Vec3::ONE);
    }

    #[test]
    fn an_edit_marks_the_scene_dirty() {
        let (mut world, _) = action_world();
        let mut changed = property(&[0], Edit::Duration).changed();
        assert!(changed(&world), "the first poll fires");
        assert!(!changed(&world));

        property(&[0], Edit::Duration).set(&mut world, &6.0f32);
        assert!(changed(&world));
    }

    #[test]
    fn the_type_picker_rewrites_the_combinator() {
        let (mut world, _) = action_world();
        let combinator = |world: &World| {
            let scene = world.resource::<EditorScene>().scene();
            match node_at(&scene.0.animation, &[1]) {
                Some(Node::Block { block, .. }) => {
                    block.combinator.clone()
                }
                _ => panic!("a block"),
            }
        };

        set_combinator(&mut world, &[1], 0);
        assert!(matches!(combinator(&world), Combinator::Chain));
        set_combinator(&mut world, &[1], 1);
        assert!(matches!(combinator(&world), Combinator::All));
        set_combinator(&mut world, &[1], 2);
        assert!(matches!(
            combinator(&world),
            Combinator::Flow(stagger) if stagger == DEFAULT_STAGGER
        ));

        set_combinator(&mut world, &[], 1);
        let scene = world.resource::<EditorScene>().scene();
        assert!(matches!(
            scene.0.animation.combinator,
            Combinator::All
        ));
    }

    #[test]
    fn the_type_picker_leaves_an_action_alone() {
        let (mut world, _) = action_world();
        set_combinator(&mut world, &[0], 2);
        assert!(matches!(node(&world, &[0]), Node::Action { .. }));
    }

    #[test]
    fn interp_choice_round_trips() {
        for interp in [None, Some(AnimInterp::Linear)] {
            let choice = InterpChoice::from(interp);
            assert_eq!(Option::<AnimInterp>::from(choice), interp);
        }
    }

    #[test]
    fn a_field_name_falls_back_to_the_last_type_segment() {
        let world = world_of(scene(Vec::new(), ValuePool::default()));
        let field = FieldRef::new("some::crate::Thing", "::a::b");
        assert_eq!(field_name(&world, &field), "Thing::a::b");
    }

    /// Opens the editor on `scene`, with `path` selected.
    fn selecting(
        scene: MotionGfxScene,
        path: Option<Vec<usize>>,
    ) -> Editor {
        let mut editor = Editor::new();
        editor.world().insert_resource(EditorScene::new(scene));
        editor.world().insert_resource(SelectedAction(path));
        editor.step(SETTLE);
        editor
    }

    #[test]
    fn the_panel_starts_empty_and_follows_the_selection() {
        let mut editor = Editor::new();
        // The inspector says it too.
        assert_eq!(editor.texts("Nothing selected").len(), 2);

        editor
            .world()
            .insert_resource(SelectedAction(Some(vec![3])));
        editor.step(SETTLE);
        editor.text("Selection is no longer in the scene");
        assert_eq!(editor.texts("Nothing selected").len(), 1);

        editor.world().insert_resource(SelectedAction(None));
        editor.step(SETTLE);
        assert!(
            editor
                .texts("Selection is no longer in the scene")
                .is_empty()
        );
        assert_eq!(editor.texts("Nothing selected").len(), 2);
    }

    #[test]
    fn a_draft_shows_its_rows() {
        let mut editor = selecting(
            scene(
                vec![draft(Some(Duration::from_secs(1)))],
                ValuePool::default(),
            ),
            Some(vec![0]),
        );
        editor.text("Duration");
        editor.text("Delay");
        assert_eq!(editor.texts("Unassigned").len(), 2);
        assert!(editor.texts("Ease").is_empty());
    }

    #[test]
    fn a_block_shows_the_type_picker_and_flow_adds_a_stagger() {
        let mut editor = selecting(
            scene(
                vec![Node::block(Block::chain(vec![draft(None)]))],
                ValuePool::default(),
            ),
            Some(vec![0]),
        );
        editor.text("Type");
        // The timeline names the block too, so there is more than
        // one.
        assert!(!editor.texts("Chain").is_empty());
        assert!(editor.texts("Stagger").is_empty());

        editor.press("Flow");
        let scene = editor
            .world()
            .resource::<EditorScene>()
            .scene()
            .0
            .animation
            .clone();
        let Some(Node::Block { block, .. }) = node_at(&scene, &[0])
        else {
            panic!("a block");
        };
        assert!(matches!(block.combinator, Combinator::Flow(_)));
        editor.text("Stagger");

        editor.press("All");
        assert!(editor.texts("Stagger").is_empty());
    }

    #[test]
    fn an_action_shows_its_subject_target_and_pickers() {
        let (values, id) = pooled(2.0);
        let mut editor = Editor::new();
        let cube = editor
            .world()
            .spawn((
                Name::new("Cube"),
                EntityUid::new(),
                Transform::default(),
            ))
            .id();
        editor.step(SETTLE);
        let uid = *editor.world().get::<EntityUid>(cube).unwrap();
        let head =
            uid.to_string().chars().take(8).collect::<String>();

        editor.world().insert_resource(EditorScene::new(scene(
            vec![Node::Action {
                delay: Some(Duration::ZERO),
                action: action(SceneUid::Entity(uid), id),
            }],
            values,
        )));
        editor
            .world()
            .insert_resource(SelectedAction(Some(vec![0])));
        editor.step(SETTLE);

        editor.text("Subject");
        editor.text(&format!("#{head}"));
        editor.text("Transform::translation::x");
        editor.text("To");
        for row in
            ["Duration", "Delay", "Ease", "Interpolation", "Value"]
        {
            editor.text(row);
        }
        // Each enum picker shows its active variant; a shut menu's
        // rows are hidden.
        editor.text("Linear");
        editor.text("Step");
    }

    #[test]
    fn typing_a_number_leaves_the_panel_standing() {
        let mut editor = selecting(
            scene(vec![draft(None)], ValuePool::default()),
            Some(vec![0]),
        );
        let duration = editor.text("Duration");

        property(&[0], Edit::Duration).set(editor.world(), &5.0f32);
        editor.step(SETTLE);
        assert_eq!(
            editor.text("Duration"),
            duration,
            "the same node, not a rebuilt one"
        );
    }
}
