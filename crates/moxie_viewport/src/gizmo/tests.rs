use core::f32::consts::{FRAC_PI_2, FRAC_PI_6};

use super::drag::{Grab, Pointing, moved, turned};
use super::*;

/// A camera at +z looking down -z, drawing one world unit as
/// `ZOOM` pixels around the middle of its viewport.
const ZOOM: f32 = 100.0;
const MIDDLE: Vec2 = Vec2::new(400.0, 300.0);

fn project(point: Vec3) -> Option<Vec2> {
    Some(MIDDLE + Vec2::new(point.x, -point.y) * ZOOM)
}

fn point_at(cursor: Vec2) -> Pointing {
    let offset = (cursor - MIDDLE) / ZOOM;
    Pointing {
        cursor,
        ray: Ray3d::new(
            Vec3::new(offset.x, -offset.y, 10.0),
            Dir3::NEG_Z,
        ),
    }
}

fn frame() -> Frame {
    Frame {
        origin: Vec3::ZERO,
        axes: Vec3::AXES,
        scale: 1.0 / ZOOM,
        look: Vec3::NEG_Z,
        facing: Quat::IDENTITY,
    }
}

/// The point `pixels` out on the unit circle point `on` of a
/// frame with `axes`, where the viewport shows it.
fn shown_at(axes: Quat, on: Vec3, pixels: f32) -> Vec2 {
    MIDDLE + (axes * on * pixels).truncate() * Vec2::new(1.0, -1.0)
}

fn turn(grab: &Grab, now: &Pointing, snap: Option<f32>) -> Quat {
    let (axis, angle) = grab.turn(now, snap).expect("it turns");
    Quat::from_axis_angle(axis, angle)
}

fn grab(handle: Handle, cursor: Vec2) -> Grab {
    Grab {
        handle,
        frame: frame(),
        pointing: point_at(cursor),
        centre: MIDDLE,
        reach: ZOOM,
    }
}

fn style() -> GizmoStyle {
    EditorTheme::default().gizmo
}

#[test]
fn the_pointer_picks_the_handle_it_is_on() {
    let pick = |mode, cursor| {
        pick(mode, &frame(), cursor, &style(), project)
    };
    let along_x = MIDDLE + Vec2::new(60.0, 2.0);
    let along_y = MIDDLE + Vec2::new(-2.0, -60.0);

    assert_eq!(
        pick(GizmoMode::Translate, along_x),
        Some(Handle::Axis(0))
    );
    assert_eq!(
        pick(GizmoMode::Scale, along_y),
        Some(Handle::Axis(1))
    );
    assert_eq!(
        pick(GizmoMode::Translate, MIDDLE),
        Some(Handle::Centre)
    );
    let between = Vec2::new(60.0, 60.0);
    assert_eq!(pick(GizmoMode::Translate, MIDDLE + between), None);
    // Inside its ring, a scale is uniform from anywhere.
    assert_eq!(
        pick(GizmoMode::Scale, MIDDLE + between),
        Some(Handle::Centre)
    );
    // The plane between x and y faces this camera.
    let offset = style().plane_offset;
    assert_eq!(
        pick(
            GizmoMode::Translate,
            MIDDLE + Vec2::new(offset, -offset)
        ),
        Some(Handle::Plane(2))
    );

    // The ring around z faces this camera: a circle a handle's
    // length out.
    let around = Vec2::from_angle(1.0);
    assert_eq!(
        pick(GizmoMode::Rotate, MIDDLE + around * style().size),
        Some(Handle::Axis(2))
    );
    // The other two are edge on, and cross at the origin.
    assert_eq!(
        pick(GizmoMode::Rotate, MIDDLE + Vec2::new(0.0, 50.0)),
        Some(Handle::Axis(0))
    );
    assert_eq!(
        pick(GizmoMode::Rotate, MIDDLE + around * style().outer_ring),
        Some(Handle::View)
    );
    assert_eq!(
        pick(GizmoMode::Rotate, MIDDLE + Vec2::splat(40.0)),
        Some(Handle::Ball)
    );
    assert_eq!(
        pick(GizmoMode::Rotate, MIDDLE + Vec2::splat(100.0)),
        None
    );
}

#[test]
fn a_ring_shows_the_half_that_faces_the_viewer() {
    let look = Vec3::new(0.3, -0.5, -1.0).normalize();
    let arc = Arc::facing(Vec3::Y, look, 0.0);
    assert!((arc.to - arc.from - PI).abs() < 1e-4);
    for point in arc.points(Vec3::ZERO, 1.0, RING_STEPS) {
        assert!(point.dot(look) < 1e-4);
        assert!(point.dot(Vec3::Y).abs() < 1e-4);
    }
    // The middle of it is the point nearest the viewer.
    let nearest = arc.at(0.0);
    assert!(nearest.dot(look) < arc.at(0.1).dot(look));
    assert!(nearest.dot(look) < arc.at(-0.1).dot(look));

    // A little more of it with some overlap.
    let more = Arc::facing(Vec3::Y, look, 0.02);
    assert!(more.to > arc.to && more.to < arc.to + 0.1);

    // All of it face on.
    let whole = Arc::facing(look, look, 0.02);
    assert!((whole.to - whole.from - 2.0 * PI).abs() < 1e-4);
}

#[test]
fn the_hidden_half_of_a_ring_is_out_of_reach() {
    // Tipped toward the viewer, so the ring around y shows as an
    // ellipse with its near half below the origin.
    let tilt = Quat::from_rotation_x(FRAC_PI_6);
    let tilted = Frame {
        axes: Vec3::AXES.map(|axis| tilt * axis),
        ..frame()
    };
    let pick = |angle: f32| {
        let on = Vec3::new(angle.cos(), 0.0, angle.sin());
        let cursor = shown_at(tilt, on, style().size);
        pick(GizmoMode::Rotate, &tilted, cursor, &style(), project)
    };
    assert_eq!(pick(0.8), Some(Handle::Axis(1)));
    assert_eq!(pick(-0.8), Some(Handle::Ball));
}

#[test]
fn a_handle_fades_as_it_turns_to_the_viewer() {
    let style = style();
    let [gone, full] = style.axis_fade;
    assert_eq!(fade(gone, style.axis_fade), 0.0);
    assert_eq!(fade(full, style.axis_fade), 1.0);
    let half = fade((gone + full) / 2.0, style.axis_fade);
    assert!((half - 0.5).abs() < 1e-4);

    // z points at this camera, and the planes x and y face out
    // of are edge on to it.
    let frame = frame();
    assert_eq!(frame.axis_fade(2, &style), 0.0);
    assert_eq!(frame.axis_fade(0, &style), 1.0);
    assert_eq!(frame.plane_fade(0, &style), 0.0);
    assert_eq!(frame.plane_fade(2, &style), 1.0);
}

#[test]
fn an_axis_drag_moves_along_its_axis_only() {
    let from = MIDDLE + Vec2::new(50.0, 0.0);
    let grab = grab(Handle::Axis(0), from);
    let now = point_at(from + Vec2::new(150.0, 80.0));

    let delta = grab.translation(&now, None).expect("it moves");
    assert!(delta.abs_diff_eq(Vec3::new(1.5, 0.0, 0.0), 1e-4));

    let snapped =
        grab.translation(&now, Some(1.0)).expect("it moves");
    assert!(snapped.abs_diff_eq(Vec3::new(2.0, 0.0, 0.0), 1e-4));
}

#[test]
fn a_centre_drag_moves_across_the_view() {
    let grab = grab(Handle::Centre, MIDDLE);
    let now = point_at(MIDDLE + Vec2::new(100.0, -200.0));

    let delta = grab.translation(&now, None).expect("it moves");
    assert!(delta.abs_diff_eq(Vec3::new(1.0, 2.0, 0.0), 1e-4));
}

#[test]
fn a_plane_drag_moves_along_its_two_axes_only() {
    // Seen from a camera off to one side of the plane.
    let ray = |x: f32, y: f32| Pointing {
        cursor: MIDDLE,
        ray: Ray3d::new(
            Vec3::new(x, y, 10.0) + Vec3::new(10.0, 0.0, 0.0),
            Dir3::new(Vec3::new(-1.0, 0.0, -1.0))
                .expect("it is not zero"),
        ),
    };
    let grab = Grab {
        pointing: ray(0.5, 0.5),
        ..grab(Handle::Plane(2), MIDDLE)
    };

    let delta =
        grab.translation(&ray(1.75, -0.5), None).expect("it moves");
    assert!(delta.abs_diff_eq(Vec3::new(1.25, -1.0, 0.0), 1e-4));

    let snapped = grab
        .translation(&ray(1.75, -0.5), Some(1.0))
        .expect("it moves");
    assert!(snapped.abs_diff_eq(Vec3::new(1.0, -1.0, 0.0), 1e-4));
}

#[test]
fn a_ring_drag_turns_the_way_the_pointer_goes() {
    // From the right of the origin to above it, as drawn.
    let grab = grab(Handle::Axis(2), MIDDLE + Vec2::new(100.0, 0.0));
    let now = point_at(MIDDLE + Vec2::new(0.0, -100.0));

    let quarter = turn(&grab, &now, None);
    assert!((quarter * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));

    // The wedge it has swept runs from under the pointer then to
    // under the pointer now.
    let (_, angle) = grab.turn(&now, None).expect("it turns");
    let (arc, radius) =
        grab.sweep(angle, &style()).expect("it has swept");
    assert_eq!(radius, style().size);
    assert!(arc.at(arc.from).abs_diff_eq(Vec3::X, 1e-4));
    assert!(arc.at(arc.to).abs_diff_eq(Vec3::Y, 1e-4));

    // Seen from behind, the same sweep turns the other way.
    let behind = Grab {
        frame: Frame {
            look: Vec3::Z,
            ..frame()
        },
        ..grab
    };
    let back = turn(&behind, &now, None);
    assert!((back * Vec3::X).abs_diff_eq(Vec3::NEG_Y, 1e-4));

    let near = point_at(MIDDLE + Vec2::new(20.0, -100.0));
    let snapped = turn(&grab, &near, Some(FRAC_PI_2));
    assert!((snapped * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));

    // The ring that faces the view turns about the line of
    // sight.
    let facing = Grab {
        handle: Handle::View,
        ..grab
    };
    let about_view = turn(&facing, &now, None);
    assert!((about_view * Vec3::X).abs_diff_eq(Vec3::Y, 1e-4));
}

#[test]
fn a_ball_drag_rolls_the_near_side_after_the_pointer() {
    let grab = grab(Handle::Ball, MIDDLE);
    let nearest = Vec3::Z;

    // A handle's length is a radian.
    let right = point_at(MIDDLE + Vec2::new(ZOOM * FRAC_PI_2, 0.0));
    let rolled = turn(&grab, &right, None);
    assert!((rolled * nearest).abs_diff_eq(Vec3::X, 1e-4));

    let down = point_at(MIDDLE + Vec2::new(0.0, ZOOM * FRAC_PI_2));
    let rolled = turn(&grab, &down, None);
    assert!((rolled * nearest).abs_diff_eq(Vec3::NEG_Y, 1e-4));

    assert!(grab.turn(&point_at(MIDDLE), None).is_none());
}

#[test]
fn a_scale_drag_grows_one_axis_or_all_of_them() {
    let from = MIDDLE + Vec2::new(0.0, -100.0);
    let axis = grab(Handle::Axis(1), from);
    let now = point_at(from + Vec2::new(30.0, -50.0));
    let factor = axis.scale(&now, None).expect("it scales");
    assert!(factor.abs_diff_eq(Vec3::new(1.0, 1.5, 1.0), 1e-4));

    let from = MIDDLE + Vec2::new(4.0, 0.0);
    let centre = grab(Handle::Centre, from);
    let now = point_at(from + Vec2::new(ZOOM, 0.0));
    let factor = centre.scale(&now, None).expect("it scales");
    assert!(factor.abs_diff_eq(Vec3::splat(2.0), 1e-4));

    let plane = grab(Handle::Plane(2), from);
    let factor = plane.scale(&now, None).expect("it scales");
    assert!(factor.abs_diff_eq(Vec3::new(2.0, 2.0, 1.0), 1e-4));
}

#[test]
fn a_world_move_lands_in_the_parents_space() {
    let parent = GlobalTransform::from(
        Transform::from_rotation(Quat::from_rotation_y(FRAC_PI_2))
            .with_scale(Vec3::splat(2.0)),
    );
    let start = Transform::from_xyz(1.0, 0.0, 0.0);
    let delta = Vec3::new(0.0, 0.0, -4.0);

    let moved = moved(&start, &parent, delta);
    let before = parent.transform_point(start.translation);
    let after = parent.transform_point(moved);
    assert!((after - before).abs_diff_eq(delta, 1e-4));
}

#[test]
fn a_world_turn_lands_in_the_parents_space() {
    let parent = GlobalTransform::from(Transform::from_rotation(
        Quat::from_rotation_y(FRAC_PI_2),
    ));
    let start = Transform::from_rotation(Quat::from_rotation_x(0.3));
    let delta = Quat::from_rotation_z(0.7);

    let turned = turned(&start, &parent, delta);
    let before = parent.rotation() * start.rotation;
    let after = parent.rotation() * turned;
    assert!(after.abs_diff_eq(delta * before, 1e-4));
}
