//! Selection of the samples drawn as markers.

use super::GraphBounds;
use super::data::Prepared;
use super::line::Work;

/// What part of a dataset markers are drawn for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointQuery {
    pub bounds: GraphBounds,
    /// Plot size in physical pixels.
    pub size_px: [f64; 2],
    /// Extra size on all sides, in physical pixels, for markers overlapping the edge.
    pub guard_px: f64,
    /// Size of a screen cell in physical pixels, `None` to keep every sample in view.
    pub cell_px: Option<f64>,
}

/// Screen cells of a [`PointQuery`]: the guarded plot divided into `cols × rows` cells,
/// anchored at its top left corner.
struct Grid {
    x0: f64,
    y1: f64,
    /// Cells per data unit.
    kx: f64,
    ky: f64,
    /// Guard in cells.
    guard: f64,
    cols: usize,
    rows: usize,
}

impl Grid {
    fn new(query: &PointQuery) -> Self {
        let cell = query.cell_px.unwrap_or(1.0);
        let [w, h] = query.size_px;
        let b = query.bounds;
        Self {
            x0: b.x()[0],
            y1: b.y()[1],
            kx: w / b.x_span() / cell,
            ky: h / b.y_span() / cell,
            guard: query.guard_px / cell,
            cols: ((w + 2.0 * query.guard_px) / cell).ceil() as usize,
            rows: ((h + 2.0 * query.guard_px) / cell).ceil() as usize,
        }
    }

    /// Column of `x`, may be outside of the grid. Nondecreasing in `x`.
    #[inline]
    fn col(&self, x: f64) -> f64 {
        ((x - self.x0) * self.kx + self.guard).floor()
    }

    /// Row of `y` (from the top), may be outside of the grid. Nonincreasing in `y`.
    #[inline]
    fn row(&self, y: f64) -> f64 {
        ((self.y1 - y) * self.ky + self.guard).floor()
    }

    #[inline]
    fn has_row(&self, row: f64) -> bool {
        row >= 0.0 && row < self.rows as f64
    }

    #[inline]
    fn has_col(&self, col: f64) -> bool {
        col >= 0.0 && col < self.cols as f64
    }

    /// Samples whose column is in the grid, `[lo, hi)`.
    fn candidates(&self, data: &Prepared, work: &mut Work) -> [usize; 2] {
        let s = &data.samples;
        let lo = s.partition_x(0..data.len(), &mut work.reads, |x| self.col(x) < 0.0);
        let cols = self.cols as f64;
        let hi = s.partition_x(lo..data.len(), &mut work.reads, |x| self.col(x) < cols);
        [lo, hi]
    }

    /// Bit index of a cell, column major so a column is contiguous.
    #[inline]
    fn bit(&self, col: f64, row: f64) -> usize {
        col as usize * self.rows + row as usize
    }
}

/// Occupied cells.
struct Bits<'a>(&'a mut [u64]);

impl Bits<'_> {
    /// Set bit `i`, returning whether it was set already.
    #[inline]
    fn test_set(&mut self, i: usize) -> bool {
        let (word, mask) = (i / 64, 1 << (i % 64));
        let was = self.0[word] & mask != 0;
        self.0[word] |= mask;
        was
    }

    /// Whether all bits in `start..=end` are set.
    fn all(&self, start: usize, end: usize) -> bool {
        let (first, last) = (start / 64, end / 64);
        let head = !0u64 << (start % 64);
        let tail = !0u64 >> (63 - end % 64);
        if first == last {
            let mask = head & tail;
            return self.0[first] & mask == mask;
        }
        self.0[first] & head == head
            && self.0[first + 1..last].iter().all(|&w| w == !0)
            && self.0[last] & tail == tail
    }
}

/// Select the samples that get a marker for `query` into `out`, in source order.
///
/// With a cell size, only the earliest sample of each occupied cell is kept, exactly as if
/// all samples in view were checked in order, but whole parts of the extrema tree are skipped
/// when they fall into cells that are already occupied. `grid` is scratch memory.
///
/// Returns the number of samples in the guarded X range.
pub(crate) fn select_points(
    data: &Prepared,
    query: &PointQuery,
    out: &mut Vec<u32>,
    grid: &mut Vec<u64>,
    work: &mut Work,
) -> usize {
    out.clear();
    let g = Grid::new(query);
    let [lo, hi] = g.candidates(data, work);
    let s = &data.samples;

    if query.cell_px.is_none() {
        work.reads += hi - lo;
        s.for_each_y(lo..hi, |i, y| {
            if g.has_row(g.row(y)) {
                out.push(i as u32);
            }
        });
        return hi - lo;
    }

    grid.clear();
    grid.resize((g.cols * g.rows).div_ceil(64), 0);
    let mut walk = Walk {
        data,
        g: &g,
        bits: Bits(grid),
        lo,
        hi,
        out,
        work,
    };
    walk.visit(1, 0, data.leaves * super::data::LEAF);
    hi - lo
}

struct Walk<'a> {
    data: &'a Prepared,
    g: &'a Grid,
    bits: Bits<'a>,
    lo: usize,
    hi: usize,
    out: &'a mut Vec<u32>,
    work: &'a mut Work,
}

impl Walk<'_> {
    /// Visit `node`, which covers the samples `[start, end)`.
    fn visit(&mut self, node: usize, start: usize, end: usize) {
        if end <= self.lo || start >= self.hi {
            return;
        }
        self.work.nodes += 1;
        let summary = self.data.tree[node];
        if summary.is_empty() {
            return;
        }
        let g = self.g;
        let (top, bottom) = (g.row(summary.y_max), g.row(summary.y_min));
        if bottom < 0.0 || top >= g.rows as f64 {
            return;
        }

        // All finite samples of the node are between its first and last one in X, so if
        // those share a column in the grid, so do all of them.
        let s = &self.data.samples;
        self.work.reads += 2;
        let col = g.col(s.x(summary.first as usize));
        let mut column_rows = None;
        if g.has_col(col) && col == g.col(s.x(summary.last as usize)) {
            if top == bottom {
                if !self.bits.test_set(g.bit(col, top)) {
                    self.out.push(summary.first);
                }
                return;
            }
            let rows = [
                g.bit(col, top.max(0.0)),
                g.bit(col, bottom.min(g.rows as f64 - 1.0)),
            ];
            if self.bits.all(rows[0], rows[1]) {
                return;
            }
            column_rows = Some(rows);
        }

        if node >= self.data.leaves {
            let range = start.max(self.lo)..end.min(self.hi).min(self.data.len());
            for i in range {
                self.work.reads += 1;
                let row = g.row(s.y(i));
                if g.has_row(row) && !self.bits.test_set(g.bit(g.col(s.x(i)), row)) {
                    self.out.push(i as u32);
                    // The rest of the leaf can only fall into occupied cells.
                    if let Some([a, b]) = column_rows
                        && self.bits.all(a, b)
                    {
                        return;
                    }
                }
            }
            return;
        }
        let mid = start + (end - start) / 2;
        self.visit(2 * node, start, mid);
        self.visit(2 * node + 1, mid, end);
    }
}

/// [`select_points`] by checking every sample in view in order. A reference for tests and
/// benchmarks.
pub(crate) fn select_points_scan(
    data: &Prepared,
    query: &PointQuery,
    out: &mut Vec<u32>,
    grid: &mut Vec<u64>,
) -> usize {
    out.clear();
    let g = Grid::new(query);
    let [lo, hi] = g.candidates(data, &mut Work::default());
    grid.clear();
    grid.resize((g.cols * g.rows).div_ceil(64), 0);
    let mut bits = Bits(grid);
    let s = &data.samples;
    for i in lo..hi {
        let row = g.row(s.y(i));
        if g.has_row(row) && (query.cell_px.is_none() || !bits.test_set(g.bit(g.col(s.x(i)), row)))
        {
            out.push(i as u32);
        }
    }
    hi - lo
}
