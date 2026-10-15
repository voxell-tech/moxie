//! Frames piped raw into an `ffmpeg` process, which encodes them.

use core::time::Duration;
use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};

use bevy::math::UVec2;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};

use crate::Frame;

/// A video format `ffmpeg` is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// H.264 in an MP4, which plays everywhere. No alpha.
    H264,
    /// ProRes 4444 in a MOV, for editing. Keeps alpha.
    ProRes4444,
    /// VP9 in a WebM. Keeps alpha.
    Vp9,
}

impl Codec {
    /// The extension of the file it is written to.
    pub fn extension(self) -> &'static str {
        match self {
            Self::H264 => "mp4",
            Self::ProRes4444 => "mov",
            Self::Vp9 => "webm",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Self::H264 => &[
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "18",
                // Padded: 4:2:0 has no odd sizes. Tagged in the
                // filter, where the encoder takes its tags from.
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2,\
                 scale=out_color_matrix=bt709:out_range=tv,\
                 format=yuv420p,setparams=colorspace=bt709:\
                 color_primaries=bt709:color_trc=bt709",
                "-movflags",
                "+faststart",
            ],
            Self::ProRes4444 => &[
                "-c:v",
                "prores_ks",
                "-profile:v",
                "4444",
                "-pix_fmt",
                "yuva444p10le",
                "-vendor",
                "apl0",
            ],
            Self::Vp9 => &[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "24",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-pix_fmt",
                "yuva420p",
            ],
        }
    }
}

/// The `ffmpeg` to encode with, if one is found: the one shipped
/// beside this program, else one on the `PATH` or where a package
/// manager puts it.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let shipped = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    let path = std::env::var_os("PATH").unwrap_or_default();
    // An app started from the desktop is not given the shell's
    // `PATH`.
    let usual = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
        .map(PathBuf::from);
    shipped
        .into_iter()
        .chain(std::env::split_paths(&path))
        .chain(usual)
        .map(|dir| dir.join(name))
        .find(|file| file.is_file())
}

/// The frame rate of frames `step` apart, as a fraction: a whole
/// number of frames a second where `step` is one rounded to the
/// nanosecond.
pub(crate) fn rate_of(step: Duration) -> (u32, u32) {
    const SECOND: u128 = 1_000_000_000;
    let nanos = step.as_nanos().clamp(1, u32::MAX as u128);
    let fps = (SECOND + nanos / 2) / nanos;
    if (SECOND + fps / 2) / fps == nanos {
        (fps as u32, 1)
    } else {
        (SECOND as u32, nanos as u32)
    }
}

pub(crate) struct Video {
    ffmpeg: PathBuf,
    codec: Codec,
    path: PathBuf,
    step: Duration,
    /// The process, until it is left to finish by itself.
    child: Option<Child>,
    pipe: Pipe,
}

/// The way into the process, which whoever writes to it holds.
enum Pipe {
    Unopened,
    Idle(ChildStdin),
    Writing(Task<(ChildStdin, std::io::Result<()>)>),
    Closing(Task<std::io::Result<ExitStatus>>),
    Closed,
}

impl Video {
    pub(crate) fn new(
        ffmpeg: PathBuf,
        codec: Codec,
        path: PathBuf,
        step: Duration,
    ) -> Self {
        Self {
            ffmpeg,
            codec,
            path,
            step,
            child: None,
            pipe: Pipe::Unopened,
        }
    }

    pub(crate) fn ready(&self) -> bool {
        matches!(self.pipe, Pipe::Unopened | Pipe::Idle(_))
    }

    pub(crate) fn write(
        &mut self,
        frame: Frame,
    ) -> Result<(), String> {
        if matches!(self.pipe, Pipe::Unopened) {
            self.open(frame.size)?;
        }
        let Pipe::Idle(mut stdin) =
            core::mem::replace(&mut self.pipe, Pipe::Closed)
        else {
            return Err("ffmpeg was given a frame out of turn".into());
        };
        // A full pipe blocks until ffmpeg has caught up.
        self.pipe =
            Pipe::Writing(IoTaskPool::get().spawn(async move {
                let result = stdin.write_all(&frame.data);
                (stdin, result)
            }));
        Ok(())
    }

    pub(crate) fn finished(&mut self) -> Result<u32, String> {
        let Pipe::Writing(task) = &mut self.pipe else {
            return Ok(0);
        };
        let Some((stdin, result)) = check_ready(task) else {
            return Ok(0);
        };
        self.pipe = Pipe::Idle(stdin);
        match result {
            Ok(()) => Ok(1),
            Err(err) => Err(self.failure(&err.to_string())),
        }
    }

    pub(crate) fn close(&mut self) -> Result<bool, String> {
        match core::mem::replace(&mut self.pipe, Pipe::Closed) {
            Pipe::Idle(stdin) => {
                // The end of its input is what tells ffmpeg to
                // finish the file.
                drop(stdin);
                if let Some(mut child) = self.child.take() {
                    self.pipe = Pipe::Closing(
                        IoTaskPool::get()
                            .spawn(async move { child.wait() }),
                    );
                }
                Ok(false)
            }
            Pipe::Closing(mut task) => {
                let Some(status) = check_ready(&mut task) else {
                    self.pipe = Pipe::Closing(task);
                    return Ok(false);
                };
                match status {
                    Ok(status) if status.success() => {
                        let _ = std::fs::remove_file(self.log());
                        Ok(true)
                    }
                    Ok(status) => {
                        Err(self.failure(&status.to_string()))
                    }
                    Err(err) => Err(self.failure(&err.to_string())),
                }
            }
            Pipe::Writing(task) => {
                self.pipe = Pipe::Writing(task);
                Ok(false)
            }
            Pipe::Unopened | Pipe::Closed => Ok(true),
        }
    }

    /// Starts ffmpeg, to read frames of `size` from its input.
    fn open(&mut self, size: UVec2) -> Result<(), String> {
        let log = File::create(self.log())
            .map_err(|err| crate::unwritten(&self.log(), err))?;
        let (frames, seconds) = rate_of(self.step);
        let mut command = Command::new(&self.ffmpeg);
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostats"])
            .args(["-y", "-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-video_size", &format!("{}x{}", size.x, size.y)])
            .args(["-framerate", &format!("{frames}/{seconds}")])
            .args(["-i", "pipe:0", "-an"])
            .args(self.codec.args())
            .arg(&self.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // To a file: a pipe nobody reads fills up, and ffmpeg
            // stops with it.
            .stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // CREATE_NO_WINDOW: no console flashes up beside the
            // editor.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|err| {
            format!(
                "could not start {}: {err}",
                self.ffmpeg.display()
            )
        })?;
        if let Some(stdin) = child.stdin.take() {
            self.pipe = Pipe::Idle(stdin);
        }
        self.child = Some(child);
        Ok(())
    }

    /// Where ffmpeg's own messages go while it runs.
    fn log(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(".log");
        name.into()
    }

    /// `error`, with what ffmpeg had to say about it.
    fn failure(&self, error: &str) -> String {
        let said =
            std::fs::read_to_string(self.log()).unwrap_or_default();
        format!("ffmpeg failed: {error}\n{}", said.trim())
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        // Still here, so the export was cancelled or failed.
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_step_is_a_whole_rate_where_it_rounds_to_one() {
        // A 24th and a 60th of a second, to the nanosecond.
        assert_eq!(
            rate_of(Duration::from_nanos(41_666_667)),
            (24, 1)
        );
        assert_eq!(
            rate_of(Duration::from_nanos(16_666_667)),
            (60, 1)
        );
        assert_eq!(rate_of(Duration::from_millis(10)), (100, 1));
        assert_eq!(
            rate_of(Duration::from_millis(300)),
            (1_000_000_000, 300_000_000),
        );
    }
}
