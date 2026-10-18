use std::ops::Range;
use std::sync::Arc;

use super::GraphError;

/// Samples per leaf of the extrema tree.
pub(crate) const LEAF: usize = 64;

/// "No sample" in a [`Summary`].
const NONE: u32 = u32::MAX;

// Samples --------------------------------------------------------------------
pub(crate) enum Samples {
    Xy(Vec<[f64; 2]>),
    Uniform {
        x_start: f64,
        x_step: f64,
        values: Vec<f64>,
    },
}

impl Samples {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Xy(points) => points.len(),
            Self::Uniform { values, .. } => values.len(),
        }
    }

    #[inline]
    pub(crate) fn x(&self, i: usize) -> f64 {
        match self {
            Self::Xy(points) => points[i][0],
            Self::Uniform {
                x_start, x_step, ..
            } => x_start + i as f64 * x_step,
        }
    }

    #[inline]
    pub(crate) fn y(&self, i: usize) -> f64 {
        match self {
            Self::Xy(points) => points[i][1],
            Self::Uniform { values, .. } => values[i],
        }
    }

    /// Call `f(index, y)` for each sample in `range`, with the storage matched only once.
    #[inline]
    pub(crate) fn for_each_y(&self, range: Range<usize>, mut f: impl FnMut(usize, f64)) {
        let start = range.start;
        match self {
            Self::Xy(points) => {
                for (i, p) in points[range].iter().enumerate() {
                    f(start + i, p[1]);
                }
            }
            Self::Uniform { values, .. } => {
                for (i, &y) in values[range].iter().enumerate() {
                    f(start + i, y);
                }
            }
        }
    }

    /// The first index in `range` for which `pred(x)` is false, assuming `pred` is true for
    /// a prefix of the (nondecreasing) X values and false after it.
    pub(crate) fn partition_x(
        &self,
        range: Range<usize>,
        reads: &mut usize,
        mut pred: impl FnMut(f64) -> bool,
    ) -> usize {
        let (mut lo, mut hi) = (range.start, range.end);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            *reads += 1;
            if pred(self.x(mid)) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Like [`Self::partition_x`], but cheap when the partition point is close to
    /// `range.start`: probes `start + 1, 3, 7, ...` before searching between the probes.
    pub(crate) fn gallop_x(
        &self,
        range: Range<usize>,
        reads: &mut usize,
        mut pred: impl FnMut(f64) -> bool,
    ) -> usize {
        let (start, end) = (range.start, range.end);
        let mut lo = start;
        let mut step = 1;
        loop {
            let probe = start.saturating_add(step);
            if probe >= end {
                return self.partition_x(lo..end, reads, pred);
            }
            *reads += 1;
            if pred(self.x(probe)) {
                lo = probe + 1;
                step = step * 2 + 1;
            } else {
                return self.partition_x(lo..probe, reads, pred);
            }
        }
    }
}

// Summary --------------------------------------------------------------------
/// Finite samples of a node of the extrema tree, by index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Summary {
    pub first: u32,
    pub last: u32,
    /// Earliest sample with the smallest Y.
    pub min: u32,
    /// Earliest sample with the largest Y.
    pub max: u32,
    pub y_min: f64,
    pub y_max: f64,
}

impl Summary {
    pub(crate) const EMPTY: Self = Self {
        first: NONE,
        last: NONE,
        min: NONE,
        max: NONE,
        y_min: f64::INFINITY,
        y_max: f64::NEG_INFINITY,
    };

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.first == NONE
    }

    /// Add the finite sample `i`, which comes after all samples added so far.
    #[inline]
    fn push(&mut self, i: u32, y: f64) {
        if self.is_empty() {
            self.first = i;
        }
        self.last = i;
        // Strict comparisons keep the earliest of equal extrema.
        if y < self.y_min {
            self.y_min = y;
            self.min = i;
        }
        if y > self.y_max {
            self.y_max = y;
            self.max = i;
        }
    }

    /// Combine `self` with `next`, whose samples all come after the ones of `self`.
    #[inline]
    pub(crate) fn then(self, next: Self) -> Self {
        if self.is_empty() {
            return next;
        }
        if next.is_empty() {
            return self;
        }
        let (min, y_min) = if next.y_min < self.y_min {
            (next.min, next.y_min)
        } else {
            (self.min, self.y_min)
        };
        let (max, y_max) = if next.y_max > self.y_max {
            (next.max, next.y_max)
        } else {
            (self.max, self.y_max)
        };
        Self {
            first: self.first,
            last: next.last,
            min,
            max,
            y_min,
            y_max,
        }
    }
}

// Prepared -------------------------------------------------------------------
pub(crate) struct Prepared {
    pub samples: Samples,
    /// Contiguous runs of finite Y, as `[start, end)` in source order.
    pub runs: Vec<[u32; 2]>,
    /// Flat binary tree: the root is node 1, the children of `n` are `2n` and `2n + 1`,
    /// and leaf `l` (samples `l * LEAF .. (l + 1) * LEAF`) is node `leaves + l`.
    pub tree: Vec<Summary>,
    /// Number of leaves, a power of two.
    pub leaves: usize,
}

impl Prepared {
    fn new(samples: Samples) -> Result<Self, GraphError> {
        let len = samples.len();
        if len >= NONE as usize {
            return Err(GraphError::TooManySamples(len));
        }
        match &samples {
            Samples::Xy(points) => {
                let mut previous = f64::NEG_INFINITY;
                for (index, p) in points.iter().enumerate() {
                    if !p[0].is_finite() {
                        return Err(GraphError::NonFiniteX { index });
                    }
                    if p[0] < previous {
                        return Err(GraphError::DescendingX { index });
                    }
                    previous = p[0];
                }
            }
            Samples::Uniform {
                x_start, x_step, ..
            } => {
                if !x_step.is_finite() || *x_step <= 0.0 {
                    return Err(GraphError::InvalidStep(*x_step));
                }
                // X grows with the index, so it's enough to check the ends.
                for index in [0, len.saturating_sub(1)].into_iter().filter(|_| len > 0) {
                    if !(x_start + index as f64 * x_step).is_finite() {
                        return Err(GraphError::NonFiniteX { index });
                    }
                }
            }
        }

        let leaves = len.div_ceil(LEAF).max(1).next_power_of_two();
        let mut tree = vec![Summary::EMPTY; 2 * leaves];
        let mut runs = Vec::new();
        let mut run_start = None;
        for leaf in 0..len.div_ceil(LEAF) {
            let range = leaf * LEAF..((leaf + 1) * LEAF).min(len);
            let summary = &mut tree[leaves + leaf];
            samples.for_each_y(range, |i, y| {
                if y.is_finite() {
                    summary.push(i as u32, y);
                    run_start.get_or_insert(i as u32);
                } else if let Some(start) = run_start.take() {
                    runs.push([start, i as u32]);
                }
            });
        }
        if let Some(start) = run_start {
            runs.push([start, len as u32]);
        }
        for node in (1..leaves).rev() {
            tree[node] = tree[2 * node].then(tree[2 * node + 1]);
        }

        Ok(Self {
            samples,
            runs,
            tree,
            leaves,
        })
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.samples.len()
    }

    /// Summary of the finite samples in `range`.
    pub(crate) fn summary(
        &self,
        range: Range<usize>,
        reads: &mut usize,
        nodes: &mut usize,
    ) -> Summary {
        let (start, end) = (range.start, range.end.min(self.len()));
        if start >= end {
            return Summary::EMPTY;
        }
        let scan = |range: Range<usize>, reads: &mut usize| {
            *reads += range.len();
            let mut s = Summary::EMPTY;
            self.samples.for_each_y(range, |i, y| {
                if y.is_finite() {
                    s.push(i as u32, y);
                }
            });
            s
        };

        // Leaves completely inside the range.
        let first_leaf = start.div_ceil(LEAF);
        let end_leaf = end / LEAF;
        if first_leaf >= end_leaf {
            return scan(start..end, reads);
        }
        let head = scan(start..first_leaf * LEAF, reads);
        let tail = scan(end_leaf * LEAF..end, reads);

        let (mut left, mut right) = (Summary::EMPTY, Summary::EMPTY);
        let (mut lo, mut hi) = (first_leaf + self.leaves, end_leaf + self.leaves);
        while lo < hi {
            if lo & 1 == 1 {
                left = left.then(self.tree[lo]);
                lo += 1;
                *nodes += 1;
            }
            if hi & 1 == 1 {
                hi -= 1;
                right = self.tree[hi].then(right);
                *nodes += 1;
            }
            lo /= 2;
            hi /= 2;
        }
        head.then(left).then(right).then(tail)
    }

    /// Indices of the finite runs overlapping `range`.
    pub(crate) fn runs_in(&self, range: Range<usize>) -> Range<usize> {
        let first = self.runs.partition_point(|r| r[1] as usize <= range.start);
        let end = self.runs.partition_point(|r| (r[0] as usize) < range.end);
        first..end.max(first)
    }
}

// GraphData ------------------------------------------------------------------
/// Drawable extent of a [`GraphData`]: the X and Y range of its samples with a finite Y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DataBounds {
    /// `[min, max]` of X.
    pub x: [f64; 2],
    /// `[min, max]` of Y.
    pub y: [f64; 2],
}

/// An immutable, prepared dataset for [`crate::Graph`].
///
/// Preparation validates the samples and builds an index of them in one linear pass, so
/// later drawing only looks at the samples it needs. Prepare datasets once, outside of the
/// per-frame UI code (it may run on another thread, `GraphData` is `Send + Sync`), and
/// replace them by preparing a new one.
///
/// Cloning is cheap: clones share the samples and the index.
///
/// - X values must be finite and nondecreasing. Duplicates are fine, and the samples are
///   never reordered.
/// - A nonfinite Y (NaN or infinite) is a gap: lines are not drawn across it.
#[derive(Clone)]
pub struct GraphData(pub(crate) Arc<Prepared>);

impl GraphData {
    /// Samples with explicit X values, `[x, y]`.
    ///
    /// The vector is moved into the dataset, not copied.
    ///
    /// # Errors
    /// If an X value is not finite or smaller than the one before it.
    pub fn from_xy(points: Vec<[f64; 2]>) -> Result<Self, GraphError> {
        Prepared::new(Samples::Xy(points)).map(|p| Self(Arc::new(p)))
    }

    /// Samples with uniformly spaced X: sample `i` is at `x_start + i * x_step`.
    ///
    /// The X values are computed when needed, not stored.
    ///
    /// # Errors
    /// If `x_step` is not finite and positive, or an X value is not finite.
    pub fn from_uniform(x_start: f64, x_step: f64, values: Vec<f64>) -> Result<Self, GraphError> {
        Prepared::new(Samples::Uniform {
            x_start,
            x_step,
            values,
        })
        .map(|p| Self(Arc::new(p)))
    }

    /// Number of samples, including gaps.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` if there are no samples.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// X of sample `index`.
    ///
    /// # Panics
    /// If `index >= len()`.
    pub fn x(&self, index: usize) -> f64 {
        assert!(index < self.len(), "sample index out of range");
        self.0.samples.x(index)
    }

    /// Y of sample `index`, nonfinite for a gap.
    ///
    /// # Panics
    /// If `index >= len()`.
    pub fn y(&self, index: usize) -> f64 {
        assert!(index < self.len(), "sample index out of range");
        self.0.samples.y(index)
    }

    /// The samples given to [`Self::from_xy`], `None` for uniform data.
    pub fn as_xy(&self) -> Option<&[[f64; 2]]> {
        match &self.0.samples {
            Samples::Xy(points) => Some(points),
            Samples::Uniform { .. } => None,
        }
    }

    /// The values given to [`Self::from_uniform`], `None` for XY data.
    pub fn as_uniform(&self) -> Option<&[f64]> {
        match &self.0.samples {
            Samples::Xy(_) => None,
            Samples::Uniform { values, .. } => Some(values),
        }
    }

    /// Extent of the samples with a finite Y, `None` if there are none.
    pub fn bounds(&self) -> Option<DataBounds> {
        let root = self.0.tree[1];
        (!root.is_empty()).then(|| DataBounds {
            x: [
                self.0.samples.x(root.first as usize),
                self.0.samples.x(root.last as usize),
            ],
            y: [root.y_min, root.y_max],
        })
    }

    /// Number of contiguous runs of samples with a finite Y.
    pub fn finite_runs(&self) -> usize {
        self.0.runs.len()
    }

    /// Indices of the samples with `x_min <= x <= x_max`.
    pub fn index_range(&self, x_min: f64, x_max: f64) -> Range<usize> {
        let samples = &self.0.samples;
        let all = 0..self.len();
        let mut reads = 0;
        let start = samples.partition_x(all.clone(), &mut reads, |x| x < x_min);
        let end = samples.partition_x(all, &mut reads, |x| x <= x_max);
        start..end.max(start)
    }

    /// Indices `[min, max]` of the smallest and largest finite Y in `range`, the earliest
    /// one if several are equal. `None` if `range` has no finite Y.
    ///
    /// Takes `O(log n + 128)` time, independent of the length of `range`.
    pub fn y_extrema(&self, range: Range<usize>) -> Option<[usize; 2]> {
        let s = self.0.summary(range, &mut 0, &mut 0);
        (!s.is_empty()).then_some([s.min as usize, s.max as usize])
    }

    /// `true` if both handles share the same prepared samples.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for GraphData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphData")
            .field("len", &self.len())
            .field("uniform", &self.as_uniform().is_some())
            .field("bounds", &self.bounds())
            .finish()
    }
}
