//! A clickable [`Frame`] around one content view.
//!
//! Like a [`Stack`](super::Stack), it holds its frame and forwards the
//! frame's builder methods. Its own defaults (fill, radius, centred
//! content) are set rules inside a scope that ends before the content
//! is built, so they beat an outer `set::<Frame>` but not the call
//! site, and never reach the content.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ui::{
    AlignItems, FlexDirection, JustifyContent, UiRect, Val,
};
use bevy::ui_widgets::Button as ButtonBehavior;

use crate::prop::Prop;
use crate::tokens::{SpacingTokens, SurfaceTokens};
use crate::views::frame::{Frame, forward_all_frame_props};
use crate::{Bevy, Cx, Styled, View};

pub struct Button<C> {
    pub frame: Frame,
    pub content: C,
}

pub fn button<C>(content: C) -> Button<C> {
    Button {
        frame: Frame::unset(),
        content,
    }
}

impl<C> Button<C> {
    pub fn direction(
        mut self,
        direction: impl Into<Prop<FlexDirection>>,
    ) -> Self {
        self.frame = self.frame.direction(direction);
        self
    }

    forward_all_frame_props!();
}

impl<T, C> View<Bevy, T> for Button<C>
where
    T: SurfaceTokens + SpacingTokens + Send + Sync + 'static,
    C: View<Bevy, T>,
{
    fn build(self, cx: &mut Cx<'_, Bevy, T>) -> Entity {
        let node = cx.scope(|cx| {
            cx.set::<Frame>(|frame, theme: &T| {
                frame
                    .fill(theme.fill())
                    .radius(theme.radius())
                    .justify(JustifyContent::Center)
                    .align(AlignItems::Center)
            });
            cx.build(self.frame)
        });
        cx.world.entity_mut(node).insert(ButtonBehavior);
        cx.under(node, |cx| cx.build(self.content));
        node
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::hierarchy::Children;
    use bevy::text::{FontSize, TextFont};
    use bevy::time::TimePlugin;
    use bevy::ui::widget::Text;
    use bevy::ui::{BackgroundColor, BorderRadius, Node, px};

    use super::*;
    use crate::tokens::{TextTokens, Tone};
    use crate::views::{Label, label};
    use crate::{AnyView, FynixProtoPlugin, Theme, mount};

    struct Plain;

    impl SpacingTokens for Plain {
        fn gap(&self) -> f32 {
            6.0
        }

        fn row(&self) -> f32 {
            20.0
        }

        fn radius(&self) -> f32 {
            3.0
        }
    }

    impl SurfaceTokens for Plain {
        fn fill(&self) -> Color {
            Color::srgb(0.2, 0.2, 0.2)
        }

        fn hover(&self) -> Color {
            Color::srgb(0.3, 0.3, 0.3)
        }

        fn panel(&self) -> Color {
            Color::BLACK
        }
    }

    impl TextTokens for Plain {
        fn tone(&self, _: Tone) -> Color {
            Color::WHITE
        }

        fn body_size(&self) -> f32 {
            14.0
        }

        fn small_size(&self) -> f32 {
            11.0
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            FynixProtoPlugin::<Plain>::default(),
        ))
        .insert_resource(Theme(Plain));
        app
    }

    fn fill(app: &App, node: Entity) -> Color {
        app.world().get::<BackgroundColor>(node).unwrap().0
    }

    #[test]
    fn a_button_defaults_from_the_theme_and_holds_its_content() {
        let mut app = app();
        let node =
            mount::<Plain>(app.world_mut(), button(label("Save")));

        assert!(app.world().get::<ButtonBehavior>(node).is_some());
        assert_eq!(fill(&app, node), Color::srgb(0.2, 0.2, 0.2));
        assert_eq!(
            app.world().get::<Node>(node).unwrap().border_radius,
            BorderRadius::all(Val::Px(3.0))
        );
        let content = app.world().get::<Children>(node).unwrap()[0];
        assert_eq!(
            app.world().get::<Text>(content).unwrap().0,
            "Save"
        );
    }

    #[test]
    fn the_call_site_beats_the_default() {
        let mut app = app();
        let node = mount::<Plain>(
            app.world_mut(),
            button(label("x")).fill(Color::WHITE),
        );

        assert_eq!(fill(&app, node), Color::WHITE);
    }

    #[test]
    fn a_set_rule_on_frame_does_not_reach_the_content() {
        let mut app = app();
        let root = mount::<Plain>(
            app.world_mut(),
            AnyView::<Bevy, Plain>::new(|cx| {
                let root = cx.spawn();
                cx.under(root, |cx| {
                    cx.set::<Frame>(|f, _| f.width(px(10.0)));
                    cx.set::<Label>(|l, _| l.size(20.0));
                    cx.build(button(label("x")));
                });
                root
            }),
        );
        let node = app.world().get::<Children>(root).unwrap()[0];

        let ui = app.world().get::<Node>(node).unwrap();
        assert_eq!(ui.width, Val::Px(10.0));
        let content = app.world().get::<Children>(node).unwrap()[0];
        assert_eq!(
            app.world().get::<TextFont>(content).unwrap().font_size,
            FontSize::Px(20.0)
        );
    }
}
