#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::perf)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::similar_names)]

use std::{
  cmp::{max, min},
  collections::HashSet,
  ffi::{c_void, CStr},
  ptr::null,
};

use const_str::cstr;
use num_traits::NumCast;
use vapoursynth4_rs::{
  core::CoreRef,
  declare_plugin,
  ffi::{VSFrame, VSVideoFormat},
  frame::{Frame, FrameContext, VideoFormat, VideoFrame},
  key,
  map::{MapMut, MapRef},
  node::{
    ActivationReason, Dependencies, Filter, FilterDependency, Node, RequestPattern, VideoNode,
  },
  utils::{is_constant_video_format, is_same_video_info},
  ColorFamily, SampleType,
};

/// Returns the peak value for the bit depth of the format specified.
const fn peak_value(format: &VSVideoFormat) -> u32 {
  match format.sample_type {
    SampleType::Float => 1,
    SampleType::Integer => (1 << format.bits_per_sample) - 1,
  }
}

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

fn normalize_planes(input: MapRef) -> Result<Vec<bool>, String> {
  let m = input.num_elements(key!("planes")).unwrap_or(-1);
  let mut process = vec![m <= 0; 3];

  for i in 0..m {
    let o = input
      .get_int_saturated(key!("planes"), i)
      .expect("Failed to read 'planes' parameter.");

    if !(0..3).contains(&o) {
      return Err(format!("Plane index {o} is out of range [0, 3)."));
    }

    if process[o as usize] {
      return Err(format!("Plane {o} is specified more than once."));
    }

    process[o as usize] = true;
  }

  Ok(process)
}

/// Grows the mask in `clipa` (`node1`) into the mask in `clipb` (`node2`). This
/// is an equivalent of the Avisynth function `mt_hysteresis`. Note that both
/// clips are are expected to be in the typical mask range which means that all
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
  process_planes: Vec<bool>,
}

impl HysteresisFilter {
  fn process_frame<T>(
    &self,
    src1: &VideoFrame,
    src2: &VideoFrame,
    dst: &mut VideoFrame,
    format: &VideoFormat,
  ) where
    T: Copy + NumCast + PartialOrd,
  {
    let (lower, upper): (T, T) = (T::from(0).unwrap(), T::from(self.peak).unwrap());

    let mut visited = HashSet::<i32>::new();

    for plane in 0..format.num_planes {
      if !self.process_planes[plane as usize] {
        continue;
      }

      let width = src1.frame_width(plane);
      let height = src1.frame_height(plane);
      let stride = src1.stride(plane) / size_of::<T>() as isize;
      let src1_ptr: *const T = src1.plane(plane).cast();
      let src2_ptr: *const T = src2.plane(plane).cast();
      let dst_ptr: *mut T = dst.plane_mut(plane).cast();

      for i in 0..stride * (height as isize) {
        unsafe {
          *dst_ptr.offset(i) = lower;
        }
      }

      let mut coords = Vec::<(i32, i32)>::new();

      for y in 0..height {
        for x in 0..width {
          let count = stride * (y as isize) + (x as isize);
          if visited.contains(&(width * y + x))
            || unsafe { *src1_ptr.offset(count) <= lower }
            || unsafe { *src2_ptr.offset(count) <= lower }
          {
            continue;
          }

          visited.insert(width * y + x);

          unsafe {
            *dst_ptr.offset(count) = upper;
          }

          coords.push((x, y));

          while let Some(pos) = coords.pop() {
            for yy in max(pos.1 - 1, 0)..=min(pos.1 + 1, height - 1) {
              for xx in max(pos.0 - 1, 0)..=min(pos.0 + 1, width - 1) {
                let count = stride * (yy as isize) + (xx as isize);
                if visited.contains(&(width * yy + xx))
                  || unsafe { *src2_ptr.offset(count) <= lower }
                {
                  continue;
                }

                visited.insert(width * yy + xx);
                unsafe {
                  *dst_ptr.offset(count) = upper;
                }
                coords.push((xx, yy));
              }
            }
          }
        }
      }
    }
  }
}

impl Filter for HysteresisFilter {
  type Error = &'static CStr;
  type FrameType = VideoFrame;
  type FilterData = ();

  fn create(
    input: MapRef<'_>,
    output: MapMut<'_>,
    _data: Option<Box<Self::FilterData>>,
    mut core: CoreRef,
  ) -> Result<(), Self::Error> {
    let Ok(node1) = input.get_video_node(key!("clipa"), 0) else {
      return Err(cstr!("Failed to get clipa."));
    };
    let Ok(node2) = input.get_video_node(key!("clipb"), 0) else {
      return Err(cstr!("Failed to get clipb."));
    };

    let n = node1.clone();
    let vi = n.info();

    if !is_constant_video_format(vi) || !is_8_to_16_or_float_format(&vi.format) {
      return Err(cstr!(
        "hysteresis: only constant format 8-16 bits integer and 32 bits float input supported"
      ));
    }

    if !is_same_video_info(vi, node2.info()) {
      return Err(cstr!(
        "hysteresis: both clips must have the same dimensions and format"
      ));
    }

    let mut filter = Self {
      node1,
      node2,
      peak: peak_value(&vi.format),
      process_planes: normalize_planes(input).expect("Failed to determine places to process."),
    };

    let deps = [
      FilterDependency {
        source: filter.node1.as_mut_ptr(),
        request_pattern: RequestPattern::StrictSpatial,
      },
      FilterDependency {
        source: filter.node2.as_mut_ptr(),
        request_pattern: if vi.num_frames <= filter.node2.info().num_frames {
          RequestPattern::StrictSpatial
        } else {
          RequestPattern::General
        },
      },
    ];

    core.create_video_filter(
      output,
      cstr!("Hysteresis"),
      vi,
      Box::new(filter),
      Dependencies::new(&deps).unwrap(),
    );

    Ok(())
  }

  fn get_frame(
    &self,
    n: i32,
    activation_reason: ActivationReason,
    _frame_data: *mut *mut c_void,
    mut ctx: FrameContext,
    core: CoreRef,
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
          .iter()
          .map(|&p| if p { null() } else { src1.as_ptr() })
          .collect::<Vec<_>>()
          .try_into()
          .unwrap();

        let mut dst = core.new_video_frame2(
          fi,
          src1.frame_width(0),
          src1.frame_height(0),
          &plane_src,
          &[0, 1, 2],
          Some(&src1),
        );

        if fi.bytes_per_sample == 1 {
          self.process_frame::<u8>(&src1, &src2, &mut dst, fi);
        } else if fi.bytes_per_sample == 2 {
          self.process_frame::<u16>(&src1, &src2, &mut dst, fi);
        } else {
          self.process_frame::<f32>(&src1, &src2, &mut dst, fi);
        }

        return Ok(Some(dst));
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
  "sgt.hysteresis",
  "hysteresis",
  "Hysteresis filter.",
  (0, 0),
  VAPOURSYNTH_API_VERSION,
  0,
  (HysteresisFilter, None)
);
