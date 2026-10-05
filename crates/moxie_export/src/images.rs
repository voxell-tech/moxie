//! Frames as PNG files, one each.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder as _};

use crate::Frame;

/// Files encoded at once.
const WRITERS: usize = 4;

pub(crate) struct Images {
    dir: PathBuf,
    stem: String,
    writing: Vec<Task<Result<(), String>>>,
}

impl Images {
    pub(crate) fn new(dir: PathBuf, stem: String) -> Self {
        Self {
            dir,
            stem,
            writing: Vec::new(),
        }
    }

    pub(crate) fn ready(&self) -> bool {
        self.writing.len() < WRITERS
    }

    pub(crate) fn write(
        &mut self,
        index: u32,
        frame: Frame,
    ) -> Result<(), String> {
        if self.writing.is_empty() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|err| crate::unwritten(&self.dir, err))?;
        }
        let path =
            self.dir.join(format!("{}_{index:05}.png", self.stem));
        self.writing.push(
            AsyncComputeTaskPool::get()
                .spawn(async move { save(&path, &frame) }),
        );
        Ok(())
    }

    pub(crate) fn finished(&mut self) -> Result<u32, String> {
        let mut finished = 0;
        let mut at = 0;
        while at < self.writing.len() {
            match check_ready(&mut self.writing[at]) {
                Some(result) => {
                    result?;
                    drop(self.writing.swap_remove(at));
                    finished += 1;
                }
                None => at += 1,
            }
        }
        Ok(finished)
    }
}

fn save(path: &Path, frame: &Frame) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|err| crate::unwritten(path, err))?;
    // Fast: frames are large, many, and often only on their way to
    // an encoder.
    PngEncoder::new_with_quality(
        BufWriter::new(file),
        CompressionType::Fast,
        FilterType::Adaptive,
    )
    .write_image(
        &frame.data,
        frame.size.x,
        frame.size.y,
        ExtendedColorType::Rgba8,
    )
    .map_err(|err| crate::unwritten(path, err))
}
