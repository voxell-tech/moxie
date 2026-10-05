//! The marks of the time axis: one every so many timesteps, and the
//! reading at the wider spaced of them.

use core::time::Duration;

use crate::{TimeStep, TimelineView};

/// Minimum spacing between ticks.
const MIN_TICK_PX: f32 = 10.0;
/// Minimum spacing between labelled ticks.
const MIN_LABEL_PX: f32 = 48.0;
/// How many times its minimum spacing a level of marks is at before
/// it is at full strength.
const FADE: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tick {
    /// Pixels from the left edge of the view.
    pub(crate) x: f32,
    pub(crate) label: Option<String>,
    /// How far in the mark has faded, from none to one.
    pub(crate) strength: f32,
    /// How far in its reading has, from none to one.
    pub(crate) reading: f32,
}

/// The spacings marks are drawn at, in timesteps, each a multiple of
/// the one before: the frames that divide a second and then seconds,
/// or one and five per power of ten for a step that is no frame.
fn ladder(timestep: TimeStep) -> Vec<u64> {
    const SECONDS: [u64; 9] =
        [1, 5, 10, 30, 60, 300, 600, 1800, 3600];
    let (frames, fps) = match timestep {
        TimeStep::Fps24 => (&[1, 2, 4, 12][..], 24),
        TimeStep::Fps25 => (&[1, 5][..], 25),
        TimeStep::Fps30 => (&[1, 5, 15][..], 30),
        TimeStep::Fps50 => (&[1, 5, 25][..], 50),
        TimeStep::Fps60 => (&[1, 5, 15, 30][..], 60),
        TimeStep::Fps120 => (&[1, 5, 10, 30, 60][..], 120),
        TimeStep::Custom(_) => {
            return (0..10)
                .flat_map(|exponent| {
                    let magnitude = 10u64.pow(exponent);
                    [magnitude, 5 * magnitude]
                })
                .collect();
        }
    };
    frames
        .iter()
        .copied()
        .chain(SECONDS.map(|seconds| seconds * fps))
        .collect()
}

/// The finest spacing of `ladder` that leaves `min_px` between marks
/// when a timestep is `px_per_step` wide.
fn spacing(ladder: &[u64], px_per_step: f32, min_px: f32) -> u64 {
    ladder
        .iter()
        .copied()
        .find(|&steps| steps as f32 * px_per_step >= min_px)
        .or(ladder.last().copied())
        .unwrap_or(1)
}

/// The reading at `steps` timesteps in, among readings `major` apart:
/// seconds and frames at a frame rate, and seconds otherwise.
fn label(steps: u64, major: u64, timestep: TimeStep) -> String {
    if let Some(fps) = timestep.fps() {
        let fps = u64::from(fps);
        let (seconds, frames) = (steps / fps, steps % fps);
        // Whole seconds apart, the frames would all read nothing.
        return if major.is_multiple_of(fps) {
            format!("{seconds}")
        } else {
            format!("{seconds}:{frames:02}")
        };
    }
    let step = timestep.duration().as_secs_f64();
    // Enough decimals to tell one mark from the next, and no more.
    let apart = major as f64 * step;
    let decimals = [1.0, 0.1, 0.01]
        .iter()
        .position(|&least| apart >= least - f64::EPSILON)
        .unwrap_or(3);
    format!("{:.decimals$}", steps as f64 * step)
}

/// Every mark visible across a timeline `width` px wide in order, a
/// whole number of `timestep`s in each. A level of marks fades in as
/// the zoom brings it apart.
pub(crate) fn ticks(
    view: &TimelineView,
    width: f32,
    timestep: TimeStep,
) -> Vec<Tick> {
    let step = timestep.duration().max(Duration::from_millis(1));
    let px_per_step = view.px_per_second * step.as_secs_f32();
    // A zero scale has no marks to give.
    if !(px_per_step.is_finite() && px_per_step > 0.0) {
        return Vec::new();
    }

    let ladder = ladder(timestep);
    let minor = spacing(&ladder, px_per_step, MIN_TICK_PX);
    let major = spacing(&ladder, px_per_step, MIN_LABEL_PX);
    let offset =
        (view.offset.as_secs_f64() / step.as_secs_f64()) as i64;
    let across = (width / px_per_step) as i64;
    // Pad the range for labels near the edges.
    let (minor_i, major_i) = (minor as i64, major as i64);
    let first = (offset - major_i).div_euclid(minor_i).max(0);
    let last = (offset + across + major_i).div_euclid(minor_i);

    (first..=last)
        .map(|index| {
            let steps = index as u64 * minor;
            // The widest spacing it is a mark of.
            let level = ladder
                .iter()
                .copied()
                .filter(|&level| level >= minor)
                .take_while(|&level| steps.is_multiple_of(level))
                .last()
                .unwrap_or(minor);
            let apart = level as f32 * px_per_step;
            let faded = |least: f32| {
                ((apart - least) / (least * (FADE - 1.0)))
                    .clamp(0.0, 1.0)
            };
            let labelled = level >= major;
            Tick {
                x: view
                    .x_from_time(step.saturating_mul(steps as u32)),
                label: labelled
                    .then(|| label(steps, major, timestep)),
                strength: faded(MIN_TICK_PX),
                reading: if labelled {
                    faded(MIN_LABEL_PX)
                } else {
                    0.0
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(px_per_second: f32) -> TimelineView {
        TimelineView {
            px_per_second,
            offset: Duration::ZERO,
        }
    }

    #[test]
    fn marks_land_on_frames_and_read_in_seconds_and_frames() {
        // A frame is 10 px wide: one mark a frame, a reading every
        // twelfth.
        let marks = ticks(&view(240.0), 400.0, TimeStep::Fps24);
        for (frame, mark) in marks.iter().enumerate() {
            assert!((mark.x - frame as f32 * 10.0).abs() < 1e-2);
            assert_eq!(mark.label.is_some(), frame % 12 == 0);
        }
        let readings = marks
            .iter()
            .filter_map(|mark| mark.label.as_deref())
            .take(3)
            .collect::<Vec<_>>();
        assert_eq!(readings, ["0:00", "0:12", "1:00"]);

        // Readings whole seconds apart leave the frames out.
        let marks = ticks(&view(60.0), 400.0, TimeStep::Fps24);
        let reading = marks.iter().find_map(|mark| {
            mark.label.as_deref().filter(|&label| label != "0")
        });
        assert_eq!(reading, Some("1"));
    }

    #[test]
    fn a_level_of_marks_fades_in_as_it_comes_apart() {
        let step = TimeStep::Custom(Duration::from_millis(10));
        // A mark every 10 ms is exactly its minimum apart here, and
        // every other one is a mark of 50 ms too.
        let marks = ticks(&view(1_000.0), 400.0, step);
        assert_eq!(marks[1].strength, 0.0);
        assert_eq!(marks[5].strength, 1.0);

        // Half again as far apart, it is half way in.
        let marks = ticks(&view(1_500.0), 400.0, step);
        assert!((marks[1].strength - 0.5).abs() < 1e-3);
    }
}
