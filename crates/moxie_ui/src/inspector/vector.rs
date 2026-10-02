//! [`Inspect`] impls for glam's float, signed, and unsigned vector
//! types.
//!
//! A vector's axes sit on one row, each behind a small tinted letter.
//!
//! Each input edits the whole vector: it reads one out, replaces a
//! component, and writes it back. So the editor needs no way to
//! address an axis on its own, and serves any [`Source`](
//! super::Source) - a component's field or a value the editor keeps
//! elsewhere.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, Number, number_field, row};
use bevy_fynix::{AnyView, Bevy, ViewExt as _};

use super::{Binding, Inspect, field_name};
use crate::theme::{EditorTheme, Palette};

/// The width of each axis's input.
const AXIS_WIDTH: f32 = 40.0;

/// A vector, by the axes an inspector edits it through.
trait Axes: FromReflect + Reflect + Send + Sync + 'static {
    /// What one axis holds.
    type Axis: Number + Default;
    /// One per axis, in order.
    const NAMES: &'static [&'static str];

    fn axis(&self, index: usize) -> Self::Axis;
    fn set_axis(&mut self, index: usize, value: Self::Axis);
}

/// Which colour an axis takes on, matching the gizmo it moves: none
/// of the engines above agree on much else, but they all tint X red,
/// Y green, Z blue.
fn axis_color(theme: &EditorTheme, name: &str) -> Color {
    let Palette {
        red, green, blue, ..
    } = theme.palette;
    match name {
        "x" => red,
        "y" => green,
        "z" => blue,
        _ => theme.color.text_dim,
    }
}

/// A vector's axes as one row of tight number inputs, each labelled
/// by a single tinted letter rather than the full field name the
/// generic struct walk would have given it.
fn axes<T: Axes>(binding: Binding) -> AnyView<Bevy, EditorTheme> {
    AnyView::new(move |cx| {
        let theme = cx.theme();
        // The base field, so an animatable axis can be dragged out on
        // its own (`translation.x`); `None` for a source the editor
        // keeps elsewhere.
        let field = binding.field().cloned();
        let mut cells = Vec::new();
        for (index, name) in T::NAMES.iter().enumerate() {
            cells.push(
                field_name(
                    field.as_ref().map(|field| field.child(name)),
                    name.to_uppercase(),
                )
                .ink(axis_color(theme, name))
                .bold(true)
                .boxed(),
            );
            cells.push(axis::<T>(binding.clone(), index).boxed());
        }
        cx.build(row(cells).align(AlignItems::Center).gap(6.0))
    })
}

/// One axis, as a number input over the whole vector.
fn axis<T: Axes>(
    binding: Binding,
    index: usize,
) -> impl bevy_fynix::View<Bevy, EditorTheme> {
    let written = binding.clone();
    number_field(
        binding.derive(move |binding, world| {
            binding
                .read::<T>(world)
                .map(|vector| vector.axis(index))
                .unwrap_or_default()
        }),
        move |world, value: T::Axis| {
            // Read, replace, write back: the source addresses the
            // vector, never one axis of it.
            let Some(mut vector) = written.read::<T>(world) else {
                return;
            };
            vector.set_axis(index, value);
            written.write(world, vector);
        },
    )
    .width(px(AXIS_WIDTH))
}

/// One vector type, by the axes it is edited through.
macro_rules! vector {
    ($ty:ty, $axis:ty, [$($name:literal),*]) => {
        impl Axes for $ty {
            type Axis = $axis;
            const NAMES: &'static [&'static str] = &[$($name),*];

            fn axis(&self, index: usize) -> $axis {
                self.to_array()[index]
            }

            fn set_axis(&mut self, index: usize, value: $axis) {
                let mut axes = self.to_array();
                axes[index] = value;
                *self = <$ty>::from_array(axes);
            }
        }

        impl Inspect for $ty {
            fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
                axes::<$ty>(binding)
            }
        }
    };
}

/// A family's 2/3/4-component types, which differ only in how many
/// axes they carry.
macro_rules! vector_family {
    ($axis:ty, [$vec2:ty, $vec3:ty, $vec4:ty]) => {
        vector!($vec2, $axis, ["x", "y"]);
        vector!($vec3, $axis, ["x", "y", "z"]);
        vector!($vec4, $axis, ["x", "y", "z", "w"]);
    };
}

vector_family!(f32, [Vec2, Vec3, Vec4]);
vector_family!(i32, [IVec2, IVec3, IVec4]);
vector_family!(u32, [UVec2, UVec3, UVec4]);

// A rotation is four axes like any other, and one tinted row reads
// far better than the folded group of floats the walk would give it.
vector!(Quat, f32, ["x", "y", "z", "w"]);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::Field;
    use crate::tests::{self, Probe};

    fn offset_editor(app: &mut App, probe: Entity) -> Entity {
        let binding =
            Binding::from(Field::of::<Probe>(probe).child("offset"));
        tests::show(app, Vec3::build(binding))
    }

    fn offset(app: &App, probe: Entity) -> Vec3 {
        app.world().get::<Probe>(probe).unwrap().offset
    }

    #[test]
    fn a_vector_is_one_input_per_axis_and_follows_the_world() {
        let (mut app, probe) = tests::probe_app();
        app.world_mut().get_mut::<Probe>(probe).unwrap().offset =
            Vec3::new(1.0, 2.0, 3.0);
        let root = offset_editor(&mut app, probe);
        assert_eq!(tests::inputs(&app, root), ["1", "2", "3"]);

        app.world_mut().get_mut::<Probe>(probe).unwrap().offset.y =
            5.5;
        app.update();
        assert_eq!(tests::inputs(&app, root), ["1", "5.5", "3"]);
    }

    #[test]
    fn scrubbing_one_axis_writes_that_axis_alone() {
        let (mut app, probe) = tests::probe_app();
        app.world_mut().get_mut::<Probe>(probe).unwrap().offset =
            Vec3::new(1.0, 2.0, 3.0);
        let root = offset_editor(&mut app, probe);

        let z = tests::all::<bevy::text::EditableText>(&app, root)[2];
        let z = app.world().get::<ChildOf>(z).unwrap().parent();
        tests::drag(&mut app, z, 10.0);

        assert_eq!(offset(&app, probe), Vec3::new(1.0, 2.0, 3.1));
    }

    #[test]
    fn an_unsigned_vector_is_whole_numbers_held_at_zero() {
        let (mut app, probe) = tests::probe_app();
        app.world_mut().get_mut::<Probe>(probe).unwrap().size =
            UVec2::new(4, 8);
        let binding =
            Binding::from(Field::of::<Probe>(probe).child("size"));
        let root = tests::show(&mut app, UVec2::build(binding));
        assert_eq!(tests::inputs(&app, root), ["4", "8"]);

        let x = tests::field_root(&app, root);
        tests::drag(&mut app, x, -50.0);
        let size = app.world().get::<Probe>(probe).unwrap().size;
        assert_eq!(size, UVec2::new(0, 8));
    }
}
