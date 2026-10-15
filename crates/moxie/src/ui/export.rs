//! The export panel: what the timeline is exported as and to, the
//! button that starts it, and how it is going.

use bevy::prelude::*;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Mods, ShortcutAppExt as _,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt as _, FrameProps as _, button, checkbox, column,
    dropdown, frame, ghost, label, number_field, primary, row,
    scroll, segmented,
};
use bevy_fynix::{
    AnyView, Bevy, ScopedExt as _, ViewExt as _, keyed, resource,
};
use moxie_export::{Export, Quality};
use moxie_ui::gaps::changing;
use moxie_ui::inspector::field_row;
use moxie_ui::theme::EditorTheme;

use crate::ProjectSettings;
use crate::export::{
    self, ExportSettings, ExportStatus, Ffmpeg, Format,
    timeline_frames,
};

/// The name the dock knows the panel by.
pub(super) const WINDOW: &str = "export";

/// The command that brings the panel up.
pub(super) const OPEN: CommandId = CommandId("file.export");

const QUALITIES: [Quality; 3] =
    [Quality::Draft, Quality::Standard, Quality::High];

pub(super) fn plugin(app: &mut App) {
    app.add_command(
        CommandSpec {
            id: OPEN,
            label: "Export",
            scope: GLOBAL,
            run: |world, _| {
                export::refind_ffmpeg(world);
                super::show_window(world, WINDOW);
            },
            enabled: |_| true,
            repeat: false,
        },
        &[Chord {
            key: KeyCode::KeyE,
            mods: Mods::PRIMARY,
        }],
    );
}

/// The export panel.
pub(super) fn panel() -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(|cx| {
        let space = cx.theme().space;
        let chevron = cx
            .world
            .resource::<AssetServer>()
            .load(moxie_ui::icons::CHEVRON);
        let control = space.row;
        let format = dropdown(
            Format::ALL.map(Format::label),
            resource::<ExportSettings, _>(|settings| {
                Format::ALL
                    .iter()
                    .position(|&format| format == settings.format)
                    .unwrap_or(0)
            }),
            chevron,
            |world, at| {
                if let Some(&format) = Format::ALL.get(at) {
                    export::set_format(world, format);
                }
            },
        )
        .width(percent(100.0))
        .height(px(control));
        let quality = keyed(
            changing(|world| settings(world).format.has_quality()),
            move |&shown| {
                if !shown {
                    return nothing();
                }
                labelled("Quality", quality_picker(control))
            },
        );
        let whole = row((
            checkbox(resource::<ExportSettings, _>(|settings| {
                settings.whole
            }))
            .on_change(set_whole),
            label("Whole timeline"),
        ))
        .gap(space.sm)
        .height(px(control))
        .align(AlignItems::Center);
        let range = keyed(
            changing(|world| settings(world).whole),
            move |&whole| {
                if whole {
                    return nothing();
                }
                labelled("Range", range(control))
            },
        );
        cx.build(
            scroll((column((
                labelled("Format", format.boxed()),
                quality,
                labelled("Frames", whole.boxed()),
                range,
                labelled(
                    "Output",
                    label(changing(describe)).tone(Tone::Dim).boxed(),
                ),
                labelled("File", file_row(control)),
                keyed(changing(needs_ffmpeg), |&missing| {
                    if !missing {
                        return nothing();
                    }
                    label(
                        "This format needs ffmpeg, and none was \
                         found. Install it and open Export again, \
                         or export a PNG sequence.",
                    )
                    .tone(Tone::Critical)
                    .boxed()
                }),
                keyed(
                    changing(|world| {
                        world.resource::<ExportStatus>().clone()
                    }),
                    move |status| actions(status.clone(), control),
                ),
            ))
            .width(percent(100.0))
            .gap(space.md),))
            .width(percent(100.0))
            .height(percent(100.0))
            .padding(UiRect::all(px(space.xl))),
        )
    })
}

fn settings(world: &World) -> &ExportSettings {
    world.resource::<ExportSettings>()
}

/// A row of `value` under a dimmed `name`.
fn labelled(
    name: &'static str,
    value: AnyView<Bevy, EditorTheme>,
) -> AnyView<Bevy, EditorTheme> {
    field_row(Some(label(name).tone(Tone::Dim).boxed()), value, 0)
        .boxed()
}

/// A row with nothing in it, for one that is hidden.
fn nothing() -> AnyView<Bevy, EditorTheme> {
    frame().boxed()
}

fn quality_picker(control: f32) -> AnyView<Bevy, EditorTheme> {
    segmented(
        ["Draft", "Standard", "High"],
        resource::<ExportSettings, _>(|settings| {
            QUALITIES
                .iter()
                .position(|&quality| quality == settings.quality)
                .unwrap_or(1)
        }),
        |world, at| {
            if let Some(&quality) = QUALITIES.get(at) {
                world.resource_mut::<ExportSettings>().quality =
                    quality;
            }
        },
    )
    .width(percent(100.0))
    .height(px(control))
    .boxed()
}

/// Exports every frame, or only those of the range, which starts as
/// the whole timeline.
fn set_whole(world: &mut World, whole: bool) {
    let frames = timeline_frames(world);
    let mut settings = world.resource_mut::<ExportSettings>();
    settings.whole = whole;
    if !whole && settings.last == 0 {
        settings.first = 0;
        settings.last = frames.saturating_sub(1);
    }
}

/// The first and last frame exported.
fn range(control: f32) -> AnyView<Bevy, EditorTheme> {
    let first = number_field::<u32>(
        resource::<ExportSettings, _>(|settings| settings.first),
        |world, first| {
            world.resource_mut::<ExportSettings>().first = first;
        },
    )
    .grow(1.0)
    .height(px(control));
    let last = number_field::<u32>(
        resource::<ExportSettings, _>(|settings| settings.last),
        |world, last| {
            world.resource_mut::<ExportSettings>().last = last;
        },
    )
    .grow(1.0)
    .height(px(control));
    row((first, label("to").tone(Tone::Dim), last))
        .width(percent(100.0))
        .gap(8.0)
        .align(AlignItems::Center)
        .boxed()
}

/// The size, frame count and rate an export comes out at.
fn describe(world: &World) -> String {
    let project = world.resource::<ProjectSettings>();
    let size = project.size();
    let step = project.timestep();
    let settings = settings(world);
    let count = timeline_frames(world);
    let frames = if settings.whole || count == 0 {
        count
    } else {
        let last = settings.last.min(count - 1);
        last - settings.first.min(last) + 1
    };
    let fps = 1.0 / step.as_secs_f64();
    format!(
        "{} x {}, {frames} frames at {fps:.4} fps",
        size.x, size.y
    )
    // A whole rate has no use for its decimals.
    .replace(".0000 fps", " fps")
}

/// The file picked, and the button that picks another.
fn file_row(control: f32) -> AnyView<Bevy, EditorTheme> {
    let file = label(changing(|world| {
        settings(world).file.as_ref().map_or_else(
            || "Asked for on export".to_string(),
            |file| file.display().to_string(),
        )
    }))
    .wrap(false)
    .tone(Tone::Dim);
    let choose = button(label("Choose..."))
        .height(px(control))
        .rules(ghost)
        .on_activate(|world| {
            export::choose_file(world);
        });
    row((
        row((file,))
            .grow(1.0)
            .min_width(px(0.0))
            .overflow(Overflow::clip_x()),
        choose,
    ))
    .width(percent(100.0))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Whether the format picked is a video, with no ffmpeg to encode it.
fn needs_ffmpeg(world: &World) -> bool {
    settings(world).format.codec().is_some()
        && world.resource::<Ffmpeg>().0.is_none()
}

/// The button that starts an export, or the progress of the one
/// running, under how the last one ended.
fn actions(
    status: ExportStatus,
    control: f32,
) -> AnyView<Bevy, EditorTheme> {
    let export = button(label("Export"))
        .height(px(control))
        .rules(primary)
        .on_activate(export::start);
    let items = match status {
        ExportStatus::Idle => vec![export.boxed()],
        ExportStatus::Running { job, .. } => {
            vec![
                label(changing(move |world| {
                    progress_text(world, job)
                }))
                .boxed(),
                progress_bar(job),
                button(label("Cancel"))
                    .height(px(control))
                    .rules(ghost)
                    .on_activate(export::cancel)
                    .boxed(),
            ]
        }
        ExportStatus::Done(file) => {
            let shown = file.clone();
            vec![
                label(format!("Saved {}", file.display()))
                    .tone(Tone::Dim)
                    .boxed(),
                row((
                    export,
                    button(label("Show in folder"))
                        .height(px(control))
                        .rules(ghost)
                        .on_activate(move |_| export::reveal(&shown)),
                ))
                .gap(8.0)
                .boxed(),
            ]
        }
        ExportStatus::Failed(error) => vec![
            label(format!("Export failed: {error}"))
                .tone(Tone::Critical)
                .boxed(),
            export.boxed(),
        ],
        ExportStatus::Cancelled => vec![
            label("Export cancelled").tone(Tone::Dim).boxed(),
            export.boxed(),
        ],
    };
    column(items).width(percent(100.0)).gap(8.0).boxed()
}

fn progress(world: &World, job: Entity) -> Option<(u32, u32)> {
    let export = world.get::<Export>(job)?;
    Some((export.written(), export.total()))
}

fn progress_text(world: &World, job: Entity) -> String {
    match progress(world, job) {
        Some((written, total)) => {
            format!("Frame {written} of {total}")
        }
        None => "Finishing".to_string(),
    }
}

/// A track filled as far as the export has written.
fn progress_bar(job: Entity) -> AnyView<Bevy, EditorTheme> {
    AnyView::<Bevy, EditorTheme>::new(move |cx| {
        let color = cx.theme().color;
        let filled = frame()
            .width(changing(move |world| {
                let share = progress(world, job)
                    .map_or(1.0, |(at, of)| {
                        at as f32 / of.max(1) as f32
                    });
                percent(share * 100.0)
            }))
            .height(percent(100.0))
            .fill(color.accent);
        cx.build(
            row((filled,))
                .width(percent(100.0))
                .height(px(6.0))
                .fill(color.fill),
        )
    })
}
