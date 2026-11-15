// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah

//! Streaming motion-compensated interpolation through the linked libavfilter.
//! Output is drained after every input; no complete video is kept in RAM.

use crate::{Error, Result, ffi};
use concat_core::{Frame, FrameRate};
use ffmpeg_the_third::{filter, format::Pixel, util::frame::video::Video};
use std::path::Path;

/// A fixed 60 FPS motion-compensated interpolator, using FFmpeg's bidirectional
/// motion estimation and adaptive overlapped block compensation, not blending.
pub struct Motion60 {
    graph: filter::Graph,
    width: u32,
    height: u32,
}

impl Motion60 {
    /// Builds the graph at the source size and rate. Rejects unsupported sizes.
    pub fn new(width: u32, height: u32, rate: FrameRate) -> Result<Self> {
        let path = Path::new("60 FPS interpolation");
        if width < 16
            || height < 16
            || !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || width.max(height) > 3840
            || width.min(height) > 2160
        {
            return Err(Error::Probe {
                path: path.into(),
                detail: "Interpolation requires even dimensions between 16 pixels and UHD 4K."
                    .into(),
            });
        }
        ffi::init();
        let mut graph = filter::Graph::new();
        // SAFETY: a new graph, before adding any filters. FFmpeg 8's filter
        // thread pool can deadlock on Windows; this job already has a worker.
        unsafe {
            (*graph.as_mut_ptr()).nb_threads = 1;
        }
        let pix: ffmpeg_the_third::sys::AVPixelFormat = Pixel::RGBA.into();
        let fps = rate.fps();
        let args = format!(
            "video_size={width}x{height}:pix_fmt={}:time_base=1/1000000:pixel_aspect=1/1:frame_rate={}/{}",
            pix.0,
            fps.numerator(),
            fps.denominator()
        );
        let find = |name| {
            filter::find(name).ok_or_else(|| Error::Missing {
                what: "filter",
                name: name.into(),
            })
        };
        graph
            .add(&find("buffer")?, "in", &args)
            .map_err(|e| ffi::fail("interpolation source", path, e))?;
        graph
            .add(&find("buffersink")?, "out", "")
            .map_err(|e| ffi::fail("interpolation sink", path, e))?;
        graph.output("in",0).and_then(|p|p.input("out",0)).and_then(|p|
            p.parse("format=yuv420p,minterpolate=fps=60:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1,format=rgba"))
            .map_err(|e|ffi::fail("motion interpolation",path,e))?;
        graph
            .validate()
            .map_err(|e| ffi::fail("motion interpolation", path, e))?;
        Ok(Self {
            graph,
            width,
            height,
        })
    }

    /// Supplies a frame at a microsecond timestamp relative to the clip's start.
    /// Drain [`Self::receive`] before supplying the next frame.
    pub fn push(&mut self, frame: &Frame, micros: i64) -> Result<()> {
        if frame.width() != self.width || frame.height() != self.height {
            return Err(Error::FrameSizeMismatch {
                want_width: self.width,
                want_height: self.height,
                got_width: frame.width(),
                got_height: frame.height(),
            });
        }
        let mut video = Video::new(Pixel::RGBA, self.width, self.height);
        let stride = video.stride(0);
        let row = self.width as usize * 4;
        for (y, line) in frame.pixels().chunks_exact(row).enumerate() {
            video.data_mut(0)[y * stride..y * stride + row].copy_from_slice(line);
        }
        video.set_pts(Some(micros));
        self.graph
            .get("in")
            .expect("source")
            .source()
            .add(&video)
            .map_err(|e| ffi::fail("interpolation input", Path::new("60 FPS"), e))
    }

    /// Signals end of input after any endpoint padding supplied by the caller.
    pub fn finish(&mut self) -> Result<()> {
        self.graph
            .get("in")
            .expect("source")
            .source()
            .flush()
            .map_err(|e| ffi::fail("interpolation flush", Path::new("60 FPS"), e))
    }

    /// One output and its timestamp in 1/60-second ticks, or none until more input.
    pub fn receive(&mut self) -> Result<Option<(Frame, i64)>> {
        let mut out = Video::empty();
        match self.graph.get("out").expect("sink").sink().frame(&mut out) {
            Ok(()) => {}
            Err(ffmpeg_the_third::Error::Eof) => return Ok(None),
            Err(e) if ffi::is_again(&e) => return Ok(None),
            Err(e) => return Err(ffi::fail("interpolation output", Path::new("60 FPS"), e)),
        }
        let tick = out.pts().ok_or_else(|| Error::Probe {
            path: "60 FPS".into(),
            detail: "Interpolated frame has no timestamp.".into(),
        })?;
        let row = self.width as usize * 4;
        let stride = out.stride(0);
        let mut bytes = Vec::with_capacity(row * self.height as usize);
        for y in 0..self.height as usize {
            bytes.extend_from_slice(&out.data(0)[y * stride..y * stride + row]);
        }
        let frame =
            Frame::from_rgba(self.width, self.height, bytes).ok_or_else(|| Error::Probe {
                path: "60 FPS".into(),
                detail: "Invalid interpolated frame dimensions.".into(),
            })?;
        Ok(Some((frame, tick)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moving_square_gets_new_intermediate_positions_at_sixty_fps() {
        let mut motion = Motion60::new(96, 64, FrameRate::from_int(15)).unwrap();
        let mut output = Vec::new();
        for i in 0..10 {
            let mut frame = Frame::black(96, 64);
            for y in 16..48 {
                for x in 8 + i * 6..24 + i * 6 {
                    let at = (y * 96 + x) * 4;
                    frame.pixels_mut()[at..at + 3].fill(230);
                }
            }
            motion
                .push(&frame, (i as f64 * 1_000_000.0 / 15.0).round() as i64)
                .unwrap();
            while let Some(out) = motion.receive().unwrap() {
                output.push(out);
            }
        }
        motion.finish().unwrap();
        while let Some(out) = motion.receive().unwrap() {
            output.push(out);
        }
        assert!(
            output.len() > 24,
            "must produce more than duplicated source frames"
        );
        assert!(output.windows(2).all(|p| p[1].1 == p[0].1 + 1));
        // The midpoint's square must move between the originals at x=8 and 14.
        let (frame, _) = output
            .iter()
            .find(|(_, tick)| *tick == 2)
            .expect("halfway at 1/30 s");
        let xs: Vec<_> = (0..96)
            .filter(|&x| frame.pixel(x, 32).unwrap()[0] > 150)
            .collect();
        let left = *xs.first().unwrap();
        assert!(
            left > 8 && left < 14,
            "motion must synthesize an intermediate position, got {left}"
        );
    }
}
