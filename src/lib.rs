use std::{
  cmp::{max, min},
  ffi::{CStr, CString, c_void},
  ptr::null,
};

use const_str::cstr;
use error::HysteresisError;
use num_traits::NumCast;
use rustc_hash::FxHashSet;
use vapours::{
  enums::ColorRange,
  frame::{VapoursVideoFrame, VapoursVideoFrameMut},
  generic::HoldsVideoFormat,
};
use vapoursynth4_rs::{
  ColorFamily, SampleType,
  core::CoreRef,
  declare_plugin,
  ffi::{VSFrame, VSVideoFormat},
  frame::{FrameContext, VideoFormat, VideoFrame, VideoFrameMut},
  key,
  map::{MapMut, MapRef},
  node::{
    ActivationReason, Dependencies, Filter, FilterDependency, Node, RequestPattern, VideoNode,
  },
  utils::{is_constant_video_format, is_same_video_info},
};

mod error;

fn is_8_to_16_or_float_format(format: &VSVideoFormat) -> bool {
  if format.color_family == ColorFamily::Undefined {
    return false;
  }

  if (format.sample_type == SampleType::Integer && format.bits_per_sample > 16)
    || (format.sample_type == SampleType::Float && format.bits_per_sample != 32)
  {
    return false;
  }

  true
}

fn normalize_planes(input: &MapRef<'_>) -> Result<[bool; 3], HysteresisError> {
  let Some(m) = input.num_elements(key!(c"planes")) else {
    return Ok([true; 3]);
  };
  let mut process = [m <= 0; 3];

  for i in 0..m {
    let Ok(o) = input.get_int_saturated(key!(c"planes"), i) else {
      return Err(HysteresisError::InvalidPlanes);
    };

    if !(0..3).contains(&o) {
      return Err(HysteresisError::PlaneOutOfRange(o));
    }

    if process[o as usize] {
      return Err(HysteresisError::DuplicatePlane(o));
    }

    process[o as usize] = true;
  }

  Ok(process)
}

/// Grows the mask in `clipa` (`node1`) into the mask in `clipb` (`node2`). This
/// is an equivalent of the Avisynth function `mt_hysteresis`. Note that both
/// clips are expected to be in the typical mask range which means that all
/// planes have to be in the 0-1 range for floating point formats.
///
/// Specifically, Hysteresis takes two bi-level masks `clipa` and `clipb` and
/// generates another bi-level mask clip. Both `clipa` and `clipb` must have the
/// same dimensions and format, and the output clip will also have that format.
///
/// If we treat the planes of the clips as representing 8-neighbourhood
/// undirected 2D grid graphs, for each of the connected components in `clipb`,
/// the whole component is copied to the output plane if and only if one of its
/// pixels is also marked in the corresponding plane from `clipa`. The argument
/// `planes` controls which planes to process, with the default being all. Any
/// unprocessed planes will be copied from the corresponding plane in `clipa`.
struct HysteresisFilter {
  node1: VideoNode,
  node2: VideoNode,

  /// Peak value.
  peak: u32,

  /// Indicates whether or not the plane at index `i` should be processed.
  process_planes: [bool; 3],
}

impl HysteresisFilter {
  fn process_frame<T>(
    &self,
    src1: &VideoFrame,
    src2: &VideoFrame,
    dst: &mut VideoFrameMut,
    format: &VideoFormat,
  ) -> Result<(), HysteresisError>
  where
    T: Copy + From<u8> + NumCast + PartialOrd,
  {
    let lower = <T as From<u8>>::from(0);
    let Some(upper) = <T as NumCast>::from(self.peak) else {
      return Err(HysteresisError::PeakOutOfRange);
    };

    // Intentionally shared across all planes to mirror miscfilters behavior.
    // https://github.com/vapoursynth/vs-miscfilters-obsolete/blob/07e0589a381f7deb3bf533bb459a94482bccc5c7/src/miscfilters.cpp#L201-L206
    let mut visited = FxHashSet::<i32>::default();

    for plane in (0..format.num_planes).filter(|&plane| self.process_planes[plane as usize]) {
      let width = src1.frame_width(plane);
      let height = src1.frame_height(plane);
      let src2_rows: Vec<&[T]> = src2.rows(plane).collect();
      let mut dst_rows: Vec<&mut [T]> = dst.rows_mut(plane).collect();

      for row in &mut dst_rows {
        row.fill(lower);
      }

      let mut coords = Vec::<(i32, i32)>::new();

      for (y, (src1_row, src2_row)) in src1.rows::<T>(plane).zip(&src2_rows).enumerate() {
        for (x, (&src1_val, &src2_val)) in src1_row.iter().zip(src2_row.iter()).enumerate() {
          if src1_val <= lower || src2_val <= lower {
            continue;
          }

          let (x, y) = (x as i32, y as i32);

          if !visited.insert(width * y + x) {
            continue;
          }

          dst_rows[y as usize][x as usize] = upper;

          coords.push((x, y));

          while let Some(pos) = coords.pop() {
            for yy in max(pos.1 - 1, 0)..=min(pos.1 + 1, height - 1) {
              for xx in max(pos.0 - 1, 0)..=min(pos.0 + 1, width - 1) {
                if visited.contains(&(width * yy + xx))
                  || src2_rows[yy as usize][xx as usize] <= lower
                {
                  continue;
                }

                visited.insert(width * yy + xx);
                dst_rows[yy as usize][xx as usize] = upper;
                coords.push((xx, yy));
              }
            }
          }
        }
      }
    }

    Ok(())
  }
}

impl Filter for HysteresisFilter {
  type Error = CString;
  type FrameType = VideoFrame;
  type FilterData = ();

  fn create(
    input: MapRef<'_>,
    mut output: MapMut<'_>,
    _data: Option<&Self::FilterData>,
    core: CoreRef<'_>,
  ) -> Result<(), Self::Error> {
    let Ok(node1) = input.get_video_node(key!(c"clipa"), 0) else {
      return Err(HysteresisError::MissingClip("clipa").into());
    };
    let Ok(node2) = input.get_video_node(key!(c"clipb"), 0) else {
      return Err(HysteresisError::MissingClip("clipb").into());
    };

    let n = node1.clone();
    let vi = n.info();

    if !is_constant_video_format(vi) || !is_8_to_16_or_float_format(&vi.format) {
      return Err(HysteresisError::UnsupportedFormat.into());
    }

    if !is_same_video_info(vi, node2.info()) {
      return Err(HysteresisError::MismatchedClips.into());
    }

    let filter = Self {
      node1,
      node2,
      peak: vi.format.peak_value(None, Some(ColorRange::Full)) as u32,
      process_planes: normalize_planes(&input)?,
    };

    let deps = [
      FilterDependency {
        source: filter.node1.as_ptr(),
        request_pattern: RequestPattern::StrictSpatial,
      },
      FilterDependency {
        source: filter.node2.as_ptr(),
        request_pattern: if vi.num_frames <= filter.node2.info().num_frames {
          RequestPattern::StrictSpatial
        } else {
          RequestPattern::General
        },
      },
    ];

    let Some(deps) = Dependencies::new(&deps) else {
      return Err(HysteresisError::TooManyDependencies.into());
    };

    core.create_video_filter(&mut output, cstr!("Hysteresis"), vi, Box::new(filter), deps);

    Ok(())
  }

  fn get_frame(
    &self,
    n: i32,
    activation_reason: ActivationReason,
    _frame_data: *mut *mut c_void,
    mut ctx: FrameContext,
    core: CoreRef<'_>,
  ) -> Result<Option<VideoFrame>, Self::Error> {
    match activation_reason {
      ActivationReason::Initial => {
        ctx.request_frame_filter(n, &self.node1);
        ctx.request_frame_filter(n, &self.node2);
      }
      ActivationReason::AllFramesReady => {
        let src1 = self.node1.get_frame_filter(n, &mut ctx);
        let src2 = self.node2.get_frame_filter(n, &mut ctx);

        let fi = src1.get_video_format();

        let plane_src: [*const VSFrame; 3] = self
          .process_planes
          .map(|p| if p { null() } else { src1.as_ptr() });

        // SAFETY: Everything is taken from `src1` and both of the planes arrays
        // have 3 entries.
        let mut dst = unsafe {
          core.new_video_frame2(
            fi,
            src1.frame_width(0),
            src1.frame_height(0),
            &plane_src,
            &[0, 1, 2],
            Some(&src1),
          )
        };

        if fi.bytes_per_sample == 1 {
          self.process_frame::<u8>(&src1, &src2, &mut dst, fi)?;
        } else if fi.bytes_per_sample == 2 {
          self.process_frame::<u16>(&src1, &src2, &mut dst, fi)?;
        } else {
          self.process_frame::<f32>(&src1, &src2, &mut dst, fi)?;
        }

        return Ok(Some(dst.freeze()));
      }
      ActivationReason::Error => {}
    }

    Ok(None)
  }

  const NAME: &'static CStr = cstr!("Hysteresis");
  const ARGS: &'static CStr = cstr!("clipa:vnode;clipb:vnode;planes:int[]:opt;");
  const RETURN_TYPE: &'static CStr = cstr!("clip:vnode;");
}

declare_plugin!(
  c"sgt.hysteresis",
  c"hysteresis",
  c"Hysteresis filter.",
  (1, 4),
  VAPOURSYNTH_API_VERSION,
  0,
  (HysteresisFilter, None)
);
