// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! Offline motion-compensated 60 FPS copies. The original is never written.

use crate::jobs::SingleFlight;
use concat_core::FrameRate;
use concat_media::{
    DecodeOptions, Decoder, EncodeOptions, Encoder, FrameSink, FrameSource, VideoCodec,
    interpolate::Motion60,
};
use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

/// One bounded worker for real motion interpolation, with progress/cancellation.
pub struct Interpolators {
    gate: Arc<SingleFlight>,
}
impl Default for Interpolators {
    fn default() -> Self {
        Self::new()
    }
}
impl Interpolators {
    /// An idle conversion service.
    pub fn new() -> Self {
        Self {
            gate: Arc::new(SingleFlight::new()),
        }
    }
    /// Whether a conversion is running.
    pub fn is_busy(&self) -> bool {
        self.gate.is_busy()
    }
    /// Cancels at the next decoded/interpolated frame or mux boundary.
    pub fn cancel(&self) {
        self.gate.cancel();
    }
    /// Converts the source into a new cached MP4, preserving its audio.
    /// Blocking: call on a worker thread, never the UI thread.
    pub fn convert(
        &self,
        source: &str,
        target: &Path,
        progress: &mut dyn FnMut(f32),
    ) -> Result<PathBuf, String> {
        let job = self.reserve()?;
        self.convert_reserved(job, source, target, progress)
    }

    /// Reserves the slot before a UI starts its background worker, so an
    /// immediate Cancel cannot be lost while the worker is still queued.
    pub fn reserve(&self) -> Result<crate::jobs::Job, String> {
        self.gate.begin("60 FPS interpolation")
    }

    /// Runs an already-reserved conversion and releases its slot on return.
    pub fn convert_reserved(
        &self,
        job: crate::jobs::Job,
        source: &str,
        target: &Path,
        progress: &mut dyn FnMut(f32),
    ) -> Result<PathBuf, String> {
        if job.cancelled() {
            return Err("interpolation cancelled".into());
        }
        if target.is_file() {
            return Ok(target.to_path_buf());
        }
        let source_meta = std::fs::metadata(source).map_err(|e| e.to_string())?;
        let source_stamp = (
            source_meta.len(),
            source_meta.modified().map_err(|e| e.to_string())?,
        );
        let info = concat_media::probe(source).map_err(|e| e.to_string())?;
        if info.audio_streams.len() > 1 {
            return Err("Interpolation of multi-stream source audio is not supported yet; the original was left unchanged.".into());
        }
        let video = info.video.ok_or("No video stream to interpolate.")?;
        if video.signal != concat_core::frame::Signal::Sdr {
            return Err("Motion interpolation currently accepts SDR video; HDR/wide-gamut source was left unchanged.".into());
        }
        if video.frame_rate.fps().as_f64() >= 60.0 {
            return Err("This source is already 60 FPS or higher.".into());
        }
        let estimated = u64::from(video.width) * u64::from(video.height) * 128 + 64 * 1024 * 1024;
        let budget = crate::memory::available().map_or(512 * 1024 * 1024, |n| n / 4 * 3);
        if estimated > budget {
            return Err("Not enough available RAM for motion interpolation at this size.".into());
        }
        log::info!(
            "interpolate: {}x{}, {} -> 60 FPS, estimated workspace {} MiB",
            video.width,
            video.height,
            video.frame_rate.fps().as_f64(),
            estimated / 1_048_576
        );
        let mut motion = Motion60::new(video.width, video.height, video.frame_rate)
            .map_err(|e| e.to_string())?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let partial = target.with_extension("motion.part.mp4");
        let muxed = target.with_extension("mux.part.mp4");
        let written = (|| -> Result<(), String> {
            let mut decoder =
                Decoder::open(source, &DecodeOptions::default().in_software().threaded(2))
                    .map_err(|e| e.to_string())?;
            let mut encoder = Encoder::create(
                &partial,
                video.width,
                video.height,
                FrameRate::SIXTY,
                &EncodeOptions {
                    codec: VideoCodec::H264,
                    hardware: true,
                    threads: 2,
                    crf: 16,
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            let period = 1.0 / video.frame_rate.fps().as_f64();
            let estimate = info.duration.map_or(1.0, |v| v.as_f64()).max(period);
            let mut origin = None;
            let mut last = None;
            let mut seen = 0usize;
            let mut produced = 0i64;
            let mut last_at = 0.0;
            let mut drain = |motion: &mut Motion60, limit: Option<i64>| -> Result<(), String> {
                while let Some((frame, tick)) = motion.receive().map_err(|e| e.to_string())? {
                    if job.cancelled() {
                        return Err("interpolation cancelled".into());
                    }
                    if limit.is_some_and(|end| tick >= end) {
                        continue;
                    }
                    if tick != produced {
                        return Err(format!(
                            "Interpolation timestamp discontinuity: expected {produced}, got {tick}."
                        ));
                    }
                    encoder.write_frame(&frame).map_err(|e| e.to_string())?;
                    produced += 1;
                    progress((produced as f64 / 60.0 / estimate).min(0.98) as f32);
                }
                Ok(())
            };
            while let Some(frame) = decoder.next_frame().map_err(|e| e.to_string())? {
                if job.cancelled() {
                    return Err("interpolation cancelled".into());
                }
                let timestamp = decoder
                    .position()
                    .map_or(seen as f64 * period, |v| v.as_f64());
                let first = *origin.get_or_insert(timestamp);
                let at = (timestamp - first).max(0.0);
                if seen > 0 && at <= last_at {
                    return Err(
                        "Non-monotonic source timestamps; interpolation was not written.".into(),
                    );
                }
                motion
                    .push(&frame, (at * 1_000_000.0).round() as i64)
                    .map_err(|e| e.to_string())?;
                drain(&mut motion, None)?;
                last = Some(frame);
                last_at = at;
                seen += 1;
            }
            let last = last.ok_or("The source has no frames.")?;
            if seen < 2 {
                return Err(
                    "At least two source frames are needed for motion interpolation.".into(),
                );
            }
            let duration = last_at + period;
            let expected = (duration * 60.0).round() as i64;
            // Lookahead beyond the source end supplies the final held sample.
            // Discard everything beyond the original video duration.
            for pad in 1..=3 {
                motion
                    .push(
                        &last,
                        ((last_at + period * f64::from(pad)) * 1_000_000.0).round() as i64,
                    )
                    .map_err(|e| e.to_string())?;
                drain(&mut motion, Some(expected))?;
            }
            motion.finish().map_err(|e| e.to_string())?;
            drain(&mut motion, Some(expected))?;
            if produced != expected {
                return Err(format!(
                    "Interpolation produced {produced} of {expected} frames; incomplete copy discarded."
                ));
            }
            encoder.finish().map_err(|e| e.to_string())?;
            drop(encoder);
            drop(decoder);
            if job.cancelled() {
                return Err("interpolation cancelled".into());
            }
            if info.audio.is_some() {
                concat_media::audio::mux(&partial, Path::new(source), &muxed)
                    .map_err(|e| e.to_string())?;
            }
            if job.cancelled() {
                return Err("interpolation cancelled".into());
            }
            let final_meta = std::fs::metadata(source).map_err(|e| e.to_string())?;
            if (
                final_meta.len(),
                final_meta.modified().map_err(|e| e.to_string())?,
            ) != source_stamp
            {
                return Err(
                    "The source changed during interpolation; the generated copy was discarded."
                        .into(),
                );
            }
            std::fs::rename(
                if info.audio.is_some() {
                    &muxed
                } else {
                    &partial
                },
                target,
            )
            .map_err(|e| e.to_string())?;
            progress(1.0);
            Ok(())
        })();
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_file(&muxed);
        written.inspect_err(|e| log::error!("interpolation failed: {e}"))?;
        Ok(target.to_path_buf())
    }
}

/// Cache identity includes the source's path, byte length and modification time.
pub fn target_for(project: &Path, source: &str) -> Option<PathBuf> {
    let meta = std::fs::metadata(source).ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    meta.len().hash(&mut hash);
    meta.modified().ok()?.hash(&mut hash);
    Some(
        project
            .join("cache")
            .join(format!("motion60-{:016x}.mp4", hash.finish())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_before_worker_start_does_not_read_or_write_files() {
        let service = Interpolators::new();
        let job = service.reserve().unwrap();
        service.cancel();
        let error = service
            .convert_reserved(
                job,
                "/does/not/exist",
                Path::new("/does/not/exist.mp4"),
                &mut |_| {},
            )
            .unwrap_err();
        assert!(error.contains("cancelled"));
        assert!(!service.is_busy());
        assert!(service.reserve().is_ok());
    }
}
