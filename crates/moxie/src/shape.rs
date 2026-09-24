//! Parametric 2D primitives: a mesh built from [`Shape2d`], drawn
//! through a material coloured by [`ShapeColor`].

use bevy::asset::AssetEventSystems;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;
use bevy_motiongfx::MotionGfxSystems;
use moxie_ui::inspector::InspectAppExt;

pub(crate) struct ShapePlugin;

impl Plugin for ShapePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (apply_geometry, apply_color)
                .after(MotionGfxSystems::Sample)
                .before(AssetEventSystems),
        );

        app.with_inspect_group("2D Mesh")
            .register_inspectable::<Shape2d>()
            .register_inspectable::<ShapeColor>();
    }
}

/// A 2D primitive, drawn through a mesh built from its kind and size.
#[derive(Component, Reflect, Clone)]
#[reflect(Component, Default, Clone)]
#[require(ShapeColor)]
#[component(on_add = attach_render, on_remove = detach_render)]
pub(crate) struct Shape2d {
    pub(crate) kind: Shape2dKind,
    pub(crate) size: Vec2,
}

impl Default for Shape2d {
    fn default() -> Self {
        Self {
            kind: Shape2dKind::default(),
            size: Vec2::splat(100.0),
        }
    }
}

impl Shape2d {
    fn mesh(&self) -> Mesh {
        let half = self.size / 2.0;
        match self.kind {
            Shape2dKind::Circle => {
                Mesh::from(Ellipse::new(half.x, half.y))
            }
            Shape2dKind::Rectangle => {
                Mesh::from(Rectangle::from_size(self.size))
            }
            Shape2dKind::Triangle => Mesh::from(Triangle2d::new(
                Vec2::new(-half.x, -half.y),
                Vec2::new(half.x, -half.y),
                Vec2::new(0.0, half.y),
            )),
        }
    }
}

/// Which primitive a [`Shape2d`] draws.
#[derive(Reflect, Clone, Copy, Default)]
pub(crate) enum Shape2dKind {
    #[default]
    Circle,
    Rectangle,
    Triangle,
}

/// The colour a [`Shape2d`] is filled with.
#[derive(Component, Reflect, Clone, Copy, Default)]
#[reflect(Component, Default, Clone)]
pub(crate) struct ShapeColor(pub(crate) Srgba);

/// Gives a new [`Shape2d`] a mesh and a material of its own.
fn attach_render(mut world: DeferredWorld, ctx: HookContext) {
    let color = world
        .get::<ShapeColor>(ctx.entity)
        .copied()
        .unwrap_or_default();
    let mesh = world.resource::<Assets<Mesh>>().reserve_handle();
    // The default blends, so any shape can fade.
    let material = world.resource_mut::<Assets<ColorMaterial>>().add(
        ColorMaterial {
            color: color.0.into(),
            ..default()
        },
    );
    world
        .commands()
        .entity(ctx.entity)
        .insert((Mesh2d(mesh), MeshMaterial2d(material)));
}

/// Removes what a [`Shape2d`] drew through along with it.
fn detach_render(mut world: DeferredWorld, ctx: HookContext) {
    world
        .commands()
        .entity(ctx.entity)
        .try_remove::<(Mesh2d, MeshMaterial2d<ColorMaterial>)>();
}

/// Builds the mesh of every [`Shape2d`] that is new or changed.
fn apply_geometry(
    mut meshes: ResMut<Assets<Mesh>>,
    q_shapes: Query<(&Shape2d, &Mesh2d), Changed<Shape2d>>,
) {
    for (shape, mesh) in &q_shapes {
        let _ = meshes.insert(&mesh.0, shape.mesh());
    }
}

/// Carries every changed [`ShapeColor`] over to its shape's material.
fn apply_color(
    mut materials: ResMut<Assets<ColorMaterial>>,
    q_colors: Query<
        (&ShapeColor, &MeshMaterial2d<ColorMaterial>),
        Changed<ShapeColor>,
    >,
) {
    for (color, material) in &q_colors {
        let color = Color::from(color.0);
        // Only a write marks the material for re-upload, so a colour
        // it already holds is left alone.
        if let Some(mut asset) = materials.get_mut(&material.0)
            && asset.color != color
        {
            asset.color = color;
        }
    }
}
