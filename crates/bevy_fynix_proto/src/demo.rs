//! The three editor call sites named in `docs/fynix_rewrite.md`,
//! rebuilt on the prototype.

pub mod asset_field;
pub mod field_row;
pub mod hierarchy;

#[cfg(test)]
mod testing {
    use bevy::prelude::*;

    use crate::tokens::{
        SpacingTokens, SurfaceTokens, TextTokens, Tone,
    };
    use crate::{FynixProtoPlugin, Theme};

    pub struct Demo;

    pub const BODY: Color = Color::WHITE;
    pub const DIM: Color = Color::srgb(0.5, 0.5, 0.5);
    pub const ACCENT: Color = Color::srgb(1.0, 0.5, 0.0);
    pub const HOVER: Color = Color::srgb(0.3, 0.3, 0.3);

    impl TextTokens for Demo {
        fn tone(&self, tone: Tone) -> Color {
            match tone {
                Tone::Body => BODY,
                Tone::Dim => DIM,
                Tone::Accent => ACCENT,
            }
        }

        fn body_size(&self) -> f32 {
            14.0
        }

        fn small_size(&self) -> f32 {
            11.0
        }
    }

    impl SurfaceTokens for Demo {
        fn fill(&self) -> Color {
            Color::srgb(0.2, 0.2, 0.2)
        }

        fn hover(&self) -> Color {
            HOVER
        }

        fn panel(&self) -> Color {
            Color::BLACK
        }
    }

    impl SpacingTokens for Demo {
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

    pub fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            FynixProtoPlugin::<Demo>::default(),
        ))
        .insert_resource(Theme(Demo));
        app
    }

    pub fn kids(app: &App, node: Entity) -> Vec<Entity> {
        app.world()
            .get::<Children>(node)
            .map(|children| children.iter().collect())
            .unwrap_or_default()
    }

    pub fn text(app: &App, node: Entity) -> String {
        app.world().get::<Text>(node).expect("a label").0.clone()
    }

    pub fn color(app: &App, node: Entity) -> Color {
        app.world().get::<TextColor>(node).expect("a label").0
    }

    pub fn fill(app: &App, node: Entity) -> Color {
        app.world().get::<BackgroundColor>(node).expect("a frame").0
    }

    pub fn ui(app: &App, node: Entity) -> &Node {
        app.world().get::<Node>(node).expect("a node")
    }
}
