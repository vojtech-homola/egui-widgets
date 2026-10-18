# Graph benchmarks

```sh
cargo bench --features bench --bench graph            # everything, ~10 minutes
cargo bench --features bench --bench graph -- engine/ # one group
```

The `bench` feature exposes the internal selection functions the benchmarks compare, it is
not meant for anything else.

Data is uniform over X `[0, 1)`: `Noise` is white noise (every pixel column spans the full Y
range, the worst case for lines and markers), `Sine` a sine with a little noise, `Gaps` a sine
with a 3-sample gap every 1000 samples. Plots are 600 points high at 1 pixel per point.

## Reference results

AMD Ryzen 9 5900X, Linux, rustc 1.99.1, release profile, single thread. Medians, rounded.
These are not enforced anywhere; rerun them on your machine before comparing.

### Preparation (`GraphData::from_*`, one linear pass)

| samples | uniform | XY |
|---|---|---|
| 100k | 0.12 ms | 0.20 ms |
| 1M | 1.2 ms | 2.0 ms |
| 10M | 12.7 ms | 21.8 ms |

### Line selection, full view at 1920 px

| samples | indexed, noise | scan, noise | indexed, gaps | scan, gaps |
|---|---|---|---|---|
| 100k | 0.27 ms | 0.32 ms | 0.18 ms | 0.31 ms |
| 1M | 0.44 ms | 2.8 ms | 0.43 ms | 2.8 ms |
| 10M | 0.51 ms | 27.5 ms | 1.95 ms | 27.4 ms |

Going from 1M to 10M samples barely changes the indexed selection of a continuous line, while
the scan grows tenfold. Gaps add work per finite run: the 10M gap dataset has 10,000 runs.
Indexed selection at 10M scales with the width: 0.21 ms at 800 px, 0.99 ms at 3840 px.

### Whole engine calls, one 10M-sample noise line

| width | pan (select + tessellate) | Y-only zoom (tessellate) | unchanged |
|---|---|---|---|
| 800 | 0.52 ms | 0.19 ms | 43 ns |
| 1920 | 1.28 ms | 0.45 ms | 41 ns |
| 3840 | 2.55 ms | 0.90 ms | 43 ns |

### Markers, full view at 1920 × 1080 px, 4 px cells

| samples | indexed, sine | scan, sine | indexed, noise | scan, noise |
|---|---|---|---|---|
| 100k | 0.95 ms | 0.99 ms | 1.6 ms | 1.5 ms |
| 1M | 2.0 ms | 8.6 ms | 6.3 ms | 9.6 ms |
| 10M | 2.7 ms | 85 ms | 6.4 ms | 86 ms |

Markers have no width bound like lines: until the cells a part of the data covers are
occupied, its samples have to be looked at. Noise covers almost every cell, which makes it
the slow case, and also gives about 100k markers, whose mesh dominates
`engine/line_points_pan_x2` (two series, line and markers, while panning: 6–12 ms).

### Repaint of cached meshes (egui batching only), 1M samples at 1920 px

| line | line + markers (sine) |
|---|---|
| 0.06 ms | 0.22 ms |
