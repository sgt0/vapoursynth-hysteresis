# vapoursynth-hysteresis

Hysteresis filter as a [VapourSynth][] plugin, with the same API as
`misc.Hysteresis()` from [vs-miscfilters-obsolete][].

Description from vs-miscfilters-obsolete:

> Grows the mask in `clipa` into the mask in `clipb`. This is an equivalent of
> the Avisynth function `mt_hysteresis`. Note that both clips are are expected
> to be in the typical mask range which means that all planes have to be in the
> 0-1 range for floating point formats.
>
> Specifically, Hysteresis takes two bi-level masks `clipa` and `clipb` and
> generates another bi-level mask clip. Both `clipa` and `clipb` must have the
> same dimensions and format, and the output clip will also have that format.
>
> If we treat the planes of the clips as representing 8-neighbourhood undirected
> 2D grid graphs, for each of the connected components in `clipb`, the whole
> component is copied to the output plane if and only if one of its pixels is
> also marked in the corresponding plane from `clipa`. The argument `planes`
> controls which planes to process, with the default being all. Any unprocessed
> planes will be copied from the corresponding plane in `clipa`.

## Install

Via [vsrepo][]:

```
vsrepo install hysteresis
```

Or manually on Windows: download a release from the [Releases][] page and unzip
`hysteresis.dll` into a [plugins directory][plugin-autoloading]. There are
separate artifacts for Raptor Lake (`*-raptorlake.zip`) and AMD Zen 4
(`*-znver4.zip`) CPUs which may or may not have better performance than the
plain x86_64 build.

## API

```python
hysteresis.Hysteresis(
    clipa: vs.VideoNode,
    clipb: vs.VideoNode,
    planes: list[int] = [0, 1, 2]
)
```

## Build

Rust v1.81.0-nightly and cargo may be used to build the project. Older versions
will likely work fine but they aren't explicitly supported.

```bash
$ git clone https://github.com/sgt0/vapoursynth-hysteresis.git
$ cd vapoursynth-hysteresis

# Debug build.
$ cargo build

# Release (optimized) build.
$ cargo build --release

# Release build optimized for the host CPU.
$ RUSTFLAGS="-C target-cpu=native" cargo build --release
```

[VapourSynth]: https://www.vapoursynth.com
[vs-miscfilters-obsolete]: https://github.com/vapoursynth/vs-miscfilters-obsolete
[vsrepo]: https://github.com/vapoursynth/vsrepo
[Releases]: https://github.com/sgt0/vapoursynth-hysteresis/releases
[plugin-autoloading]: https://www.vapoursynth.com/doc/installation.html#plugin-autoloading
