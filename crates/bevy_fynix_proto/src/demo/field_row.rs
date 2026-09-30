//! The inspector's field row: a label column and a value column.

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::entity::Entity;
    use bevy::ecs::resource::Resource;
    use bevy::text::{LineBreak, TextLayout};
    use bevy::ui::Val;

    use crate::demo::testing::{
        ACCENT, DIM, Demo, app, color, kids, text, ui,
    };
    use crate::views::{
        AnimatedField, HasAction, field_row, label, row,
    };
    use crate::{Bevy, mount};

    fn linebreak(app: &App, node: Entity) -> LineBreak {
        app.world().get::<TextLayout>(node).unwrap().linebreak
    }

    /// Wide text a value column might hold.
    const LONG: &str = "a value that could wrap";

    #[test]
    fn the_columns_split_forty_sixty() {
        let mut app = app();
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(label("Subject"), label(LONG)),
        );
        let [name, value] = kids(&app, node)[..] else {
            panic!("a label and a value");
        };

        assert_eq!(ui(&app, name).width, Val::Percent(40.0));
        assert_eq!(ui(&app, value).flex_grow, 1.0);
        assert_eq!(text(&app, name), "Subject");
    }

    #[test]
    fn depth_indents_the_row() {
        let mut app = app();
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(label("a"), label("b")).depth(2),
        );

        assert_eq!(ui(&app, node).padding.left, Val::Px(24.0));
    }

    #[test]
    fn the_label_is_forced_to_one_line_but_the_value_is_not() {
        let mut app = app();
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(label("Subject"), label(LONG)),
        );
        let [name, value] = kids(&app, node)[..] else {
            panic!("a label and a value");
        };

        assert_eq!(linebreak(&app, name), LineBreak::NoWrap);
        assert_eq!(linebreak(&app, value), LineBreak::WordBoundary);
    }

    #[test]
    fn an_explicit_wrap_on_the_caller_beats_the_rule() {
        let mut app = app();
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(label("Subject").wrap(true), label(LONG)),
        );
        let name = kids(&app, node)[0];

        assert_eq!(linebreak(&app, name), LineBreak::WordBoundary);
    }

    #[test]
    fn the_rule_ends_with_the_row() {
        let mut app = app();
        let root = mount::<Demo>(
            app.world_mut(),
            crate::AnyView::<Bevy, Demo>::new(|cx| {
                let root = cx.spawn();
                cx.under(root, |cx| {
                    cx.build(field_row(label("a"), label("b")));
                    cx.build(label("after"));
                });
                root
            }),
        );
        let after = kids(&app, root)[1];

        assert_eq!(linebreak(&app, after), LineBreak::WordBoundary);
    }

    #[test]
    fn a_composite_as_the_value_is_built_whole() {
        let mut app = app();
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(label("v"), row((label("x"), label("y")))),
        );
        let value = kids(&app, node)[1];

        assert_eq!(kids(&app, value).len(), 2);
    }

    fn animatable_row(app: &mut App) -> Entity {
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(
                label("translation")
                    .tone(crate::Prop::Value(
                        crate::tokens::Tone::Dim,
                    ))
                    .animatable("translation"),
                label("0"),
            ),
        );
        kids(app, node)[0]
    }

    #[test]
    fn an_animatable_label_records_its_field() {
        let mut app = app();
        let name = animatable_row(&mut app);

        assert_eq!(
            app.world().get::<AnimatedField>(name),
            Some(&AnimatedField("translation"))
        );
        assert_eq!(color(&app, name), DIM);
        assert_eq!(linebreak(&app, name), LineBreak::NoWrap);
    }

    #[test]
    fn has_action_turns_the_label_accent_over_the_callers_tone() {
        let mut app = app();
        let name = animatable_row(&mut app);

        app.world_mut().entity_mut(name).insert(HasAction);
        app.update();
        assert_eq!(color(&app, name), ACCENT);

        app.world_mut().entity_mut(name).remove::<HasAction>();
        app.update();
        assert_eq!(color(&app, name), DIM);
    }

    #[test]
    fn the_callers_tone_stays_bound_beside_the_state_rule() {
        use crate::resource;
        use crate::tokens::Tone;

        #[derive(Resource)]
        struct Muted(bool);

        let mut app = app();
        app.insert_resource(Muted(true));
        let node = mount::<Demo>(
            app.world_mut(),
            field_row(
                label("t")
                    .tone(resource::<Muted, _>(|muted| {
                        if muted.0 { Tone::Dim } else { Tone::Body }
                    }))
                    .animatable("t"),
                label("0"),
            ),
        );
        let name = kids(&app, node)[0];
        assert_eq!(color(&app, name), DIM);

        app.world_mut().entity_mut(name).insert(HasAction);
        app.update();
        assert_eq!(color(&app, name), ACCENT);

        app.world_mut().resource_mut::<Muted>().0 = false;
        app.world_mut().entity_mut(name).remove::<HasAction>();
        app.update();
        assert_eq!(color(&app, name), crate::demo::testing::BODY);
    }
}
