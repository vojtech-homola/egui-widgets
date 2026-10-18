//! Selection of the samples drawn as a line.

use super::data::Prepared;

/// Separates the fragments of a line selection: samples are connected only within one.
pub(crate) const BREAK: u32 = u32::MAX;

/// What part of a dataset a line is drawn for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LineQuery {
    /// Visible `[min, max]` of X.
    pub x: [f64; 2],
    /// Plot width in physical pixels.
    pub width_px: f64,
    /// Extra width on both sides, in physical pixels, for the stroke and antialiasing.
    pub guard_px: f64,
    /// Keep every sample instead of reducing to the extrema of each pixel column.
    pub exact: bool,
}

/// Work done by a selection, for statistics.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Work {
    /// Raw samples read.
    pub reads: usize,
    /// Extrema tree nodes visited.
    pub nodes: usize,
}

/// The guarded X range of `query` as sample indices, `[lo, hi)`.
pub(crate) fn candidates(data: &Prepared, query: &LineQuery, work: &mut Work) -> [usize; 2] {
    let [x0, x1] = query.x;
    let guard = query.guard_px * (x1 - x0) / query.width_px;
    let s = &data.samples;
    let lo = s.partition_x(0..data.len(), &mut work.reads, |x| x < x0 - guard);
    let hi = s.partition_x(lo..data.len(), &mut work.reads, |x| x <= x1 + guard);
    [lo, hi]
}

/// Select the samples of the line for `query` into `out`: indices in source order, with
/// [`BREAK`] after each fragment (a finite run, or the part of one in view).
///
/// The samples just outside the guarded X range are included, so segments crossing its
/// edges are drawn. Unless the query is exact or there are few samples, each pixel column
/// keeps only its first, smallest, largest and last sample.
///
/// Returns the number of samples in the guarded X range.
pub(crate) fn select_line(
    data: &Prepared,
    query: &LineQuery,
    out: &mut Vec<u32>,
    work: &mut Work,
) -> usize {
    out.clear();
    let [lo, hi] = candidates(data, query, work);
    let start = lo.saturating_sub(1);
    let end = (hi + 1).min(data.len());
    if start >= end {
        return 0;
    }

    let runs = data.runs_in(start..end);
    let columns = (query.width_px + 2.0 * query.guard_px).ceil() as usize + 2;
    let budget = 4 * columns + 2 * runs.len();
    let reduce = !query.exact && end - start > budget;

    let s = &data.samples;
    let [x0, x1] = query.x;
    let per_x = query.width_px / (x1 - x0);
    let column = |x: f64| ((x - x0) * per_x).floor();

    for &[run_start, run_end] in &data.runs[runs] {
        let a = (run_start as usize).max(start);
        let b = (run_end as usize).min(end);
        if !reduce {
            out.extend(a as u32..b as u32);
        } else {
            let mut i = a;
            while i < b {
                work.reads += 1;
                let c = column(s.x(i));
                let j = s.gallop_x(i + 1..b, &mut work.reads, |x| column(x) <= c);
                if j - i <= 4 {
                    out.extend(i as u32..j as u32);
                } else {
                    let m = data.summary(i..j, &mut work.reads, &mut work.nodes);
                    let mut keep = [i as u32, m.min, m.max, (j - 1) as u32];
                    keep.sort_unstable();
                    out.push(keep[0]);
                    for w in keep.windows(2) {
                        if w[1] != w[0] {
                            out.push(w[1]);
                        }
                    }
                }
                i = j;
            }
        }
        out.push(BREAK);
    }
    hi - lo
}

/// [`select_line`] without the extrema tree and galloping search: scans every sample in
/// view. A reference for tests and benchmarks.
pub(crate) fn select_line_scan(data: &Prepared, query: &LineQuery, out: &mut Vec<u32>) -> usize {
    out.clear();
    let mut work = Work::default();
    let [lo, hi] = candidates(data, query, &mut work);
    let start = lo.saturating_sub(1);
    let end = (hi + 1).min(data.len());
    if start >= end {
        return 0;
    }
    let runs = data.runs_in(start..end);
    let columns = (query.width_px + 2.0 * query.guard_px).ceil() as usize + 2;
    let reduce = !query.exact && end - start > 4 * columns + 2 * runs.len();

    let s = &data.samples;
    let [x0, x1] = query.x;
    let per_x = query.width_px / (x1 - x0);
    let column = |x: f64| ((x - x0) * per_x).floor();
    for &[run_start, run_end] in &data.runs[runs] {
        let a = (run_start as usize).max(start);
        let b = (run_end as usize).min(end);
        let mut i = a;
        while i < b {
            let c = column(s.x(i));
            let mut j = i;
            let (mut min, mut max) = (i, i);
            while j < b && column(s.x(j)) == c {
                if s.y(j) < s.y(min) {
                    min = j;
                }
                if s.y(j) > s.y(max) {
                    max = j;
                }
                j += 1;
            }
            if !reduce || j - i <= 4 {
                out.extend(i as u32..j as u32);
            } else {
                let mut keep = [i as u32, min as u32, max as u32, (j - 1) as u32];
                keep.sort_unstable();
                out.push(keep[0]);
                for w in keep.windows(2) {
                    if w[1] != w[0] {
                        out.push(w[1]);
                    }
                }
            }
            i = j;
        }
        out.push(BREAK);
    }
    hi - lo
}
