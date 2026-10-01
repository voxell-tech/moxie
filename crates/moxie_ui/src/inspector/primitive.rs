//! [`Inspect`] impls for the primitive types the inspector edits out
//! of the box.

use core::time::Duration;

use bevy::prelude::*;
use bevy_fynix::views::{Number, checkbox, number_field};
use bevy_fynix::{AnyView, Bevy, View, ViewExt as _};

use super::{Binding, Inspect};
use crate::theme::EditorTheme;

/// A checkbox. It never toggles itself: what it shows follows the
/// value it edits, and only moves once the write has landed.
impl Inspect for bool {
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
        let written = binding.clone();
        checkbox(binding.signal::<bool>())
            .on_change(move |world, checked| {
                written.write(world, checked);
            })
            .boxed()
    }
}

/// A number field for a numeric leaf.
pub(super) fn number<N>(
    binding: Binding,
) -> impl View<Bevy, EditorTheme>
where
    N: Number + FromReflect + PartialReflect + Default,
{
    let written = binding.clone();
    number_field(binding.signal::<N>(), move |world, value: N| {
        written.write(world, value);
    })
}

/// Implements [`Inspect`] for a numeric type the number field edits
/// as itself.
macro_rules! number_editor {
    ($($ty:ty),*) => {$(
        impl Inspect for $ty {
            fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
                number::<$ty>(binding).boxed()
            }
        }
    )*};
}

number_editor!(f32, f64, i32, i64, u32, u64);

/// A length of time, edited as seconds.
impl Inspect for Duration {
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
        let written = binding.clone();
        number_field(
            binding
                .signal::<Duration>()
                .map(|time| time.as_secs_f32()),
            move |world, secs: f32| {
                written.write(
                    world,
                    Duration::from_secs_f32(secs.max(0.0)),
                );
            },
        )
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use bevy::ui::Checked;
    use bevy::ui_widgets::ValueChange;

    use super::*;
    use crate::inspector::Field;
    use crate::tests::{self, Probe};

    fn probe_field(app: &mut App, path: &str) -> (Entity, Binding) {
        let probe = app.world_mut().spawn(Probe::default()).id();
        let field = Field::of::<Probe>(probe).child(path);
        (probe, Binding::from(field))
    }

    fn probe_of(app: &App, probe: Entity) -> Probe {
        app.world().get::<Probe>(probe).unwrap().clone()
    }

    #[test]
    fn a_checkbox_shows_follows_and_writes_the_world() {
        let (mut app, _) = tests::probe_app();
        let (probe, binding) = probe_field(&mut app, "on");
        let node = tests::show(&mut app, bool::build(binding));
        let checked =
            |app: &App| app.world().get::<Checked>(node).is_some();
        assert!(!checked(&app));

        app.world_mut().get_mut::<Probe>(probe).unwrap().on = true;
        app.update();
        assert!(checked(&app));

        app.world_mut().trigger(ValueChange {
            source: node,
            value: false,
            is_final: true,
        });
        app.update();
        assert!(!probe_of(&app, probe).on, "the write landed");
        assert!(!checked(&app), "and the box followed it");
    }

    #[test]
    fn a_number_shows_follows_and_scrubs_the_world() {
        let (mut app, _) = tests::probe_app();
        let (probe, binding) = probe_field(&mut app, "level");
        app.world_mut().get_mut::<Probe>(probe).unwrap().level = 2.5;
        let root = tests::show(&mut app, f32::build(binding));
        assert_eq!(tests::inputs(&app, root), ["2.5"]);

        app.world_mut().get_mut::<Probe>(probe).unwrap().level = 4.0;
        app.update();
        assert_eq!(tests::inputs(&app, root), ["4"]);

        let field = tests::field_root(&app, root);
        tests::drag(&mut app, field, 10.0);
        assert_eq!(probe_of(&app, probe).level, 4.1);
        assert_eq!(tests::inputs(&app, root), ["4.1"]);
    }

    #[test]
    fn scrubbing_a_number_rebuilds_nothing() {
        let (mut app, _) = tests::probe_app();
        let (_, binding) = probe_field(&mut app, "level");
        let root = tests::show(&mut app, f32::build(binding));
        let before = tests::below(&app, root);

        let field = tests::field_root(&app, root);
        tests::drag(&mut app, field, 10.0);
        tests::drag(&mut app, field, 20.0);

        assert_eq!(tests::below(&app, root), before);
    }

    #[test]
    fn a_duration_is_edited_as_seconds() {
        let (mut app, _) = tests::probe_app();
        let (probe, binding) = probe_field(&mut app, "time");
        app.world_mut().get_mut::<Probe>(probe).unwrap().time =
            Duration::from_millis(1500);
        let root = tests::show(&mut app, Duration::build(binding));
        assert_eq!(tests::inputs(&app, root), ["1.5"]);

        let field = tests::field_root(&app, root);
        tests::drag(&mut app, field, -1000.0);
        assert_eq!(
            probe_of(&app, probe).time,
            Duration::from_secs(0)
        );
    }
}
