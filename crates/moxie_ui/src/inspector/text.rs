//! [`Inspect`] impls for the text the inspector edits.

use bevy::prelude::*;
use bevy_fynix::views::{FrameProps as _, text_field};
use bevy_fynix::{AnyView, Bevy, ViewExt as _};

use super::{Binding, Inspect};
use crate::theme::EditorTheme;

/// The text input's width, a little narrower than a row's value
/// column.
const WIDTH: f32 = 110.0;

/// A single-line text input, committed on Enter or when focus leaves.
impl Inspect for String {
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
        let written = binding.clone();
        text_field(binding.signal::<String>(), move |world, text| {
            written.write(world, text);
        })
        .width(px(WIDTH))
        .boxed()
    }
}

impl Inspect for Name {
    fn build(binding: Binding) -> AnyView<Bevy, EditorTheme> {
        let written = binding.clone();
        text_field(
            binding
                .signal::<Name>()
                .map(|name| name.as_str().to_string()),
            move |world, text| {
                written.write(world, Name::new(text));
            },
        )
        .width(px(WIDTH))
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspector::Field;
    use crate::tests::{self, Probe};

    #[test]
    fn a_text_field_shows_and_follows_the_world() {
        let (mut app, probe) = tests::probe_app();
        app.world_mut().get_mut::<Probe>(probe).unwrap().name =
            "ada".into();
        let binding =
            Binding::from(Field::of::<Probe>(probe).child("name"));
        let root = tests::show(&mut app, String::build(binding));
        assert_eq!(tests::inputs(&app, root), ["ada"]);

        app.world_mut().get_mut::<Probe>(probe).unwrap().name =
            "grace".into();
        app.update();
        assert_eq!(tests::inputs(&app, root), ["grace"]);
    }

    #[test]
    fn a_name_is_edited_as_its_text() {
        let mut app = tests::app();
        let entity = app.world_mut().spawn(Name::new("cube")).id();
        let binding = Binding::from(Field::of::<Name>(entity));
        let root = tests::show(&mut app, Name::build(binding));
        assert_eq!(tests::inputs(&app, root), ["cube"]);

        app.world_mut().entity_mut(entity).insert(Name::new("cone"));
        app.update();
        assert_eq!(tests::inputs(&app, root), ["cone"]);
    }
}
