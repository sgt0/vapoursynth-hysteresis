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
> component is copied to the output plane if and only if one of its pixels is also
> marked in the corresponding plane from `clipa`. The argument `planes` controls
> which planes to process, with the default being all. Any unprocessed planes will
> be copied from the corresponding plane in `clipa`.

## API

```python
hysteresis.Hysteresis(
    clipa: vs.VideoNode,
    clipb: vs.VideoNode,
    planes: list[int] = [0, 1, 2]
)
```

[VapourSynth]: https://www.vapoursynth.com
[vs-miscfilters-obsolete]: https://github.com/vapoursynth/vs-miscfilters-obsolete
