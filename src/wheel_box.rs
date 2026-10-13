use egui::{
    Align2, Atom, AtomExt as _, AtomLayout, Atoms, Color32, CursorIcon, Event, FontSelection,
    Frame, Id, IntoAtoms, Key, Margin, Response, RichText, Sense, TextWrapMode, Ui, Vec2, Widget,
    WidgetInfo, response::Flags, text_edit, vec2,
};

/// Inner margin shared by the focused [`text_edit::TextEdit`] and the unfocused layout,
/// so the widget keeps its size and the text doesn't move when it gains focus.
const MARGIN: Margin = Margin::symmetric(4, 3);

// WheelBoxValue --------------------------------------------------------------
/// A number that can be edited with [`WheelBox`].
///
/// Implemented for all fixed-size integers and for `f32` / `f64`.
pub trait WheelBoxValue: Copy + PartialEq + Send + Sync + 'static {
    /// Decimals shown when [`WheelBox::decimals`] is not set. Ignored by integers.
    const DEFAULT_DECIMALS: u32;

    fn one() -> Self;
    /// Halve the step. Integer steps never go below 1.
    fn div2(self) -> Self;
    /// Double the step. Integer steps saturate.
    fn mul2(self) -> Self;
    /// `self + step`, saturating for integers.
    fn step_up(self, step: Self) -> Self;
    /// `self - step`, saturating for integers.
    fn step_down(self, step: Self) -> Self;
    /// Apply a mouse wheel movement in `[-1, 1]`. Integers step by the whole `step`
    /// in the wheel direction, floats scale `step` by `wheel`.
    fn wheel(self, step: Self, wheel: f32) -> Self;
    fn clamp_to(self, range: [Self; 2]) -> Self;
    /// Parse user input. Floats accept `,` as a decimal separator and must be finite.
    fn parse(text: &str) -> Option<Self>;
    /// Format for display. Integers ignore `decimals`.
    fn format(self, decimals: u32) -> String;
}

fn clamp_value<T: WheelBoxValue>(value: T, range: Option<[T; 2]>) -> T {
    match range {
        Some(range) => value.clamp_to(range),
        None => value,
    }
}

fn parse_value<T: WheelBoxValue>(text: &str, range: Option<[T; 2]>) -> Option<T> {
    T::parse(text.trim()).map(|v| clamp_value(v, range))
}

fn step_value<T: WheelBoxValue>(value: T, step: T, up: bool, range: Option<[T; 2]>) -> T {
    let v = if up {
        value.step_up(step)
    } else {
        value.step_down(step)
    };
    clamp_value(v, range)
}

fn wheel_value<T: WheelBoxValue>(value: T, step: T, wheel: f32, range: Option<[T; 2]>) -> T {
    clamp_value(value.wheel(step, wheel), range)
}

/// `==` that also treats NaN as equal to itself, so a NaN set from outside is not
/// taken for a new change on every frame.
#[allow(clippy::eq_op)]
fn same<T: PartialEq>(a: T, b: T) -> bool {
    a == b || (a != a && b != b)
}

// State ----------------------------------------------------------------------
#[derive(Clone)]
struct WheelBoxState<T> {
    /// The bound value as last seen or written, to detect changes from outside.
    value: T,
    /// The value being edited. Ahead of `value` after stepping with
    /// [`WheelBox::always_update`] off.
    state_value: T,
    string: String,
    /// `string` was typed into since it was last set from `state_value`.
    edited: bool,
    /// The pass in which the widget was last shown focused.
    edit_pass: Option<u64>,
    single_step: T,
}

impl<T: WheelBoxValue> WheelBoxState<T> {
    fn new(value: T, single_step: T, decimals: u32) -> Self {
        Self {
            value,
            state_value: value,
            string: value.format(decimals),
            edited: false,
            edit_pass: None,
            single_step,
        }
    }

    /// Set the value being edited and replace the text with it.
    fn set_value(&mut self, value: T, decimals: u32) {
        self.state_value = value;
        self.string = value.format(decimals);
        self.edited = false;
    }

    /// Drop any pending edit and show the bound `value`.
    fn reset(&mut self, value: T, decimals: u32) {
        self.value = value;
        self.set_value(value, decimals);
    }

    /// Follow a change of the bound value from outside. It replaces the value being
    /// edited, but not the text being typed.
    fn sync(&mut self, value: T, decimals: u32) {
        self.value = value;
        self.state_value = value;
        if !self.edited {
            self.string = value.format(decimals);
        }
    }

    /// The typed number if the text was edited, otherwise the value being edited
    /// at full precision. `None` if the typed text is not a number.
    fn edited_value(&self, range: Option<[T; 2]>) -> Option<T> {
        if self.edited {
            parse_value(&self.string, range)
        } else {
            Some(self.state_value)
        }
    }

    /// Finish editing and write the edited value to `value`. Invalid text is dropped.
    /// Returns whether `value` changed.
    fn commit(&mut self, value: &mut T, range: Option<[T; 2]>, decimals: u32) -> bool {
        let changed = match self.edited_value(range) {
            Some(v) if !same(v, *value) => {
                *value = v;
                true
            }
            _ => false,
        };
        self.reset(*value, decimals);
        self.edit_pass = None;
        changed
    }
}

#[derive(Default)]
struct WheelsBoxKeys {
    q: bool,
    w: bool,
    r: bool,
    up: bool,
    down: bool,
}

impl WheelsBoxKeys {
    /// Q halves the step, W doubles it and R resets it to `default`.
    fn update_step<T: WheelBoxValue>(&self, mut step: T, default: T) -> T {
        if self.q {
            step = step.div2();
        }
        if self.w {
            step = step.mul2();
        }
        if self.r {
            step = default;
        }
        step
    }
}

// WheelBox -------------------------------------------------------------------
/// Numeric input box for integers and floats.
///
/// Click to focus, then step the value with the arrow keys or the mouse wheel,
/// change the step with Q (halve), W (double) and R (reset), or type a number
/// and press Enter. Typed text is also committed when the box loses focus, and
/// discarded on Escape.
///
/// The value is kept at full precision, [`WheelBox::decimals`] only limits what is
/// shown. [`Response::changed`] is set only when the value changed.
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # use egui_widgets::WheelBox;
/// # let (mut count, mut exposure) = (1_u32, 1.0_f64);
/// ui.add(WheelBox::new(&mut count).range([0, 100]).prefix("n: "));
/// ui.add(WheelBox::new(&mut exposure).decimals(2).suffix(egui::RichText::new("ms").weak()));
/// # });
/// ```
pub struct WheelBox<'a, T> {
    default_single_step: Option<T>,
    decimals: Option<u32>,
    always_update: bool,
    range: Option<[T; 2]>,
    prefix: Atoms<'static>,
    suffix: Atoms<'static>,
    text_color: Option<Color32>,

    value: &'a mut T,
    single_step: Option<&'a mut T>,

    desired_width: Option<f32>,
}

impl<'a, T> WheelBox<'a, T> {
    pub fn new(value: &'a mut T) -> Self {
        Self {
            default_single_step: None,
            decimals: None,
            always_update: true,
            range: None,
            prefix: Atoms::default(),
            suffix: Atoms::default(),
            text_color: None,
            value,
            single_step: None,
            desired_width: None,
        }
    }

    /// Number of decimals shown for floats (default 3). Ignored by integers.
    pub fn decimals(mut self, decimals: u32) -> Self {
        self.decimals = Some(decimals);
        self
    }

    pub fn range(mut self, range: [T; 2]) -> Self {
        self.range = Some(range);
        self
    }

    /// Color of the value while not editing. Prefix and suffix keep their own color.
    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Shown before the value, also while editing, e.g. a label `"x: "` or an icon.
    pub fn prefix(mut self, prefix: impl IntoAtoms<'static>) -> Self {
        self.prefix = prefix.into_atoms();
        self
    }

    /// Shown at the right edge, also while editing, e.g. a unit `"ms"`
    /// or `RichText::new("ms").weak()`.
    ///
    /// If the box is too narrow, the suffix is hidden first, then the prefix, and only
    /// then is the value truncated.
    pub fn suffix(mut self, suffix: impl IntoAtoms<'static>) -> Self {
        self.suffix = suffix.into_atoms();
        self
    }

    pub fn desired_width(mut self, width: f32) -> Self {
        self.desired_width = Some(width);
        self
    }

    /// Write stepped values right away (default). If off, the value is written when
    /// editing ends, like typed text.
    pub fn always_update(mut self, always_update: bool) -> Self {
        self.always_update = always_update;
        self
    }

    pub fn default_single_step(mut self, default_single_step: T) -> Self {
        self.default_single_step = Some(default_single_step);
        self
    }

    pub fn single_step(mut self, single_step: &'a mut T) -> Self {
        self.single_step = Some(single_step);
        self
    }
}

impl<'a, T> Widget for WheelBox<'a, T>
where
    T: WheelBoxValue,
{
    fn ui(self, ui: &mut Ui) -> Response {
        let Self {
            default_single_step,
            decimals,
            always_update,
            range,
            prefix,
            suffix,
            text_color,
            value,
            single_step,
            desired_width,
        } = self;

        let id = ui.next_auto_id();
        // Not clamped to the available width: in a `Grid` that would pin the column
        // to its initial width. The `TextEdit` is forced to the same width via `min_size`.
        let width = desired_width.unwrap_or_else(|| ui.available_width());
        let decimals = decimals.unwrap_or(T::DEFAULT_DECIMALS);

        let has_focus = ui.memory(|m| m.has_focus(id));

        let default_single_step = default_single_step.unwrap_or(T::one());
        let mut state = ui
            .data(|m| m.get_temp(id))
            .unwrap_or_else(|| WheelBoxState::new(*value, default_single_step, decimals));

        let text_color = text_color.unwrap_or(ui.visuals().widgets.active.text_color());

        // Changed from outside, e.g. by the application or another widget showing
        // the same value.
        if !same(state.value, *value) {
            state.sync(*value, decimals);
        }

        // is focused -----------------------------------------------------
        if has_focus {
            let keys = check_keys(ui);
            // Fit to the value, not to the text being typed, so the prefix and suffix
            // don't come and go while typing.
            let affixes = fit_affixes(
                ui,
                width,
                prefix,
                &state.state_value.format(decimals),
                suffix,
            );
            let text_id = ui.next_auto_id();
            let tex_edit = text_edit::TextEdit::singleline(&mut state.string)
                .desired_width(width)
                .min_size(vec2(width, 0.))
                .clip_text(true)
                .margin(MARGIN)
                .prefix(affixes.prefix)
                .suffix(affixes.suffix)
                .id(text_id);

            let mut response = tex_edit.ui(ui);
            state.edited |= response.changed();
            // `changed()` reports changes of the value, not of the text being typed.
            response.flags.remove(Flags::CHANGED);

            if response.lost_focus() {
                if state.commit(value, range, decimals) {
                    response.mark_changed();
                }

                ui.data_mut(|m| {
                    m.insert_temp(response.id, state);
                });

                text_edit::TextEditState::default().store(ui.ctx(), text_id);

                return response;
            }
            state.edit_pass = Some(ui.ctx().cumulative_pass_nr());

            let single_step = if let Some(single_step) = single_step {
                *single_step = keys.update_step(*single_step, default_single_step);
                *single_step
            } else {
                state.single_step = keys.update_step(state.single_step, default_single_step);
                state.single_step
            };

            // Step from the typed number, if there is a valid one.
            let start = state.edited_value(range).unwrap_or(state.state_value);
            let new_value = if keys.up || keys.down {
                Some(step_value(start, single_step, keys.up, range))
            } else if response.hovered() {
                check_wheel(ui).map(|wheel| wheel_value(start, single_step, wheel, range))
            } else {
                None
            };

            if let Some(v) = new_value {
                state.set_value(v, decimals);
                if always_update && !same(v, *value) {
                    *value = v;
                    state.value = v;
                    response.mark_changed();
                }
            }

            ui.data_mut(|m| {
                m.insert_temp(response.id, state);
            });

            response

        // is not focused --------------------------------------------------
        } else {
            let mut save_state = false;
            let mut changed = false;

            // The focus was taken before the text field could report losing it, e.g. by
            // a widget shown above this one. Commit as on Enter, unless it was Escape,
            // or the widget was hidden since it was focused.
            if let Some(pass) = state.edit_pass {
                let cancel = ui.input(|i| i.key_pressed(Key::Escape))
                    || ui.ctx().cumulative_pass_nr() > pass + 1;
                if cancel {
                    state.edit_pass = None;
                } else {
                    changed = state.commit(value, range, decimals);
                }
                text_edit::TextEditState::default().store(ui.ctx(), id);
                save_state = true;
            }

            // Drop edits that were not committed.
            let text = value.format(decimals);
            if !same(state.state_value, *value) || state.string != text || state.edited {
                state.reset(*value, decimals);
                save_state = true;
            }

            let affixes = fit_affixes(ui, width, prefix, &text, suffix);
            let mut response = show_unfocused(ui, id, width, affixes, text, text_color);
            if changed {
                response.mark_changed();
            }

            if save_state {
                ui.data_mut(|m| {
                    m.insert_temp(id, state);
                });
            }

            response
        }
    }
}

// Helpers -------------------------------------------------------------------
/// Prefix and suffix that fit next to the value.
struct Affixes {
    prefix: Atoms<'static>,
    suffix: Atoms<'static>,
}

/// Hide the suffix, then the prefix, if they don't fit next to the value in `width`,
/// so the value is truncated only when it doesn't fit on its own.
fn fit_affixes(
    ui: &Ui,
    width: f32,
    prefix: Atoms<'static>,
    value_text: &str,
    suffix: Atoms<'static>,
) -> Affixes {
    let available = width - MARGIN.sum().x;
    let value_width = atom_width(ui, RichText::new(value_text).into());
    let prefix_width = affix_width(ui, &prefix);
    let suffix_width = affix_width(ui, &suffix);

    if prefix_width + value_width + suffix_width <= available {
        Affixes { prefix, suffix }
    } else if prefix_width + value_width <= available {
        Affixes {
            prefix,
            suffix: Atoms::default(),
        }
    } else {
        Affixes {
            prefix: Atoms::default(),
            suffix: Atoms::default(),
        }
    }
}

/// Natural (unwrapped) width of an atom.
fn atom_width(ui: &Ui, atom: Atom<'_>) -> f32 {
    atom.into_sized(
        ui,
        Vec2::INFINITY,
        Some(TextWrapMode::Extend),
        FontSelection::default(),
    )
    .size
    .x
}

/// Width taken by prefix or suffix atoms, including one gap per atom to its
/// neighbour (see `AtomLayout::gap`).
fn affix_width(ui: &Ui, atoms: &Atoms<'_>) -> f32 {
    let gap = ui.spacing().icon_spacing;
    atoms
        .iter()
        .map(|atom| atom_width(ui, atom.clone()) + gap)
        .sum()
}

/// Show the value between the prefix and suffix with the same geometry as the focused
/// [`text_edit::TextEdit`], and expose it to AccessKit as a text input, so the widget
/// can be found by its value (e.g. in `egui_kittest`).
fn show_unfocused(
    ui: &mut Ui,
    id: Id,
    width: f32,
    affixes: Affixes,
    value_text: String,
    text_color: Color32,
) -> Response {
    let Affixes { prefix, suffix } = affixes;

    // The value takes the free space (like the text in `TextEdit`), so the suffix sits at
    // the right edge in both states. It is truncated only if it doesn't fit on its own.
    let mut atoms = prefix;
    atoms.push_right(
        RichText::new(value_text.as_str())
            .color(text_color)
            .atom_grow(true)
            .atom_shrink(true)
            .atom_align(Align2::LEFT_CENTER),
    );
    atoms.extend_right(suffix);

    let font_id = FontSelection::default().resolve(ui.style());
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
    let height = row_height + ui.spacing().extra_text_line_spacing + MARGIN.sum().y;

    let mut allocated = AtomLayout::new(atoms)
        .id(id)
        .sense(Sense::click_and_drag())
        .frame(Frame::new().inner_margin(MARGIN))
        .min_size(vec2(width, height))
        .max_width(width)
        .align2(Align2::LEFT_CENTER)
        .wrap_mode(TextWrapMode::Truncate)
        .allocate(ui);

    // Same frame adjustments as `TextEdit`, with the regular widget colors.
    let visuals = ui.style().interact(&allocated.response);
    allocated.frame = allocated
        .frame
        .fill(visuals.bg_fill)
        .corner_radius(visuals.corner_radius)
        .stroke(visuals.bg_stroke)
        .inner_margin(
            MARGIN + Margin::same((visuals.expansion - visuals.bg_stroke.width).round() as i8),
        )
        .outer_margin(Margin::same(-(visuals.expansion as i8)));

    let response = allocated
        .paint(ui)
        .response
        .on_hover_cursor(CursorIcon::Text);

    if response.clicked() || response.drag_started() {
        response.request_focus();
    }

    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::text_edit(enabled, &value_text, &value_text, ""));

    response
}

/// Returns the vertical wheel direction clamped to `[-1, 1]`, if the wheel was used.
fn check_wheel(ui: &Ui) -> Option<f32> {
    ui.input_mut(|inp| {
        inp.smooth_scroll_delta = Vec2::ZERO;
        inp.events.iter().find_map(|event| match event {
            Event::MouseWheel { delta, .. } => Some(delta.y.clamp(-1., 1.)),
            _ => None,
        })
    })
}

fn check_keys(ui: &mut Ui) -> WheelsBoxKeys {
    ui.input_mut(|inp| {
        let q = inp.consume_key(egui::Modifiers::NONE, egui::Key::Q);
        let w = inp.consume_key(egui::Modifiers::NONE, egui::Key::W);
        let r = inp.consume_key(egui::Modifiers::NONE, egui::Key::R);

        // Keep the step shortcuts from being typed into the text field.
        if q || w || r {
            inp.events.retain(
                |event| !matches!(event, Event::Text(text) if text == "q" || text == "w" || text == "r"),
            );
        }

        WheelsBoxKeys {
            q,
            w,
            r,
            up: inp.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            down: inp.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
        }
    })
}

// Implementations -----------------------------------------------------------
macro_rules! impl_wheel_box_value {
    ($($t:ty),*) => {
        $(
            impl WheelBoxValue for $t {
                const DEFAULT_DECIMALS: u32 = 0;

                #[inline]
                fn one() -> Self {
                    1
                }

                #[inline]
                fn div2(self) -> Self {
                    let v = self / 2;
                    v.max(1)
                }

                #[inline]
                fn mul2(self) -> Self {
                    self.saturating_mul(2)
                }

                #[inline]
                fn step_up(self, step: Self) -> Self {
                    self.saturating_add(step)
                }

                #[inline]
                fn step_down(self, step: Self) -> Self {
                    self.saturating_sub(step)
                }

                #[inline]
                fn wheel(self, step: Self, wheel: f32) -> Self {
                    if wheel > 0.0 {
                        self.step_up(step)
                    } else if wheel < 0.0 {
                        self.step_down(step)
                    } else {
                        self
                    }
                }

                #[inline]
                fn clamp_to(self, range: [Self; 2]) -> Self {
                    self.clamp(range[0], range[1])
                }

                #[inline]
                fn parse(text: &str) -> Option<Self> {
                    text.parse().ok()
                }

                #[inline]
                fn format(self, _decimals: u32) -> String {
                    self.to_string()
                }
            }
        )*
    };
}

impl_wheel_box_value!(i64, i32, i16, i8, u64, u32, u16, u8);

macro_rules! impl_wheel_box_value_f {
    ($($t:ty),*) => {
        $(
            impl WheelBoxValue for $t {
                const DEFAULT_DECIMALS: u32 = 3;

                #[inline]
                fn one() -> Self {
                    1.
                }

                #[inline]
                fn div2(self) -> Self {
                    self / 2.
                }

                #[inline]
                fn mul2(self) -> Self {
                    self * 2.
                }

                #[inline]
                fn step_up(self, step: Self) -> Self {
                    self + step
                }

                #[inline]
                fn step_down(self, step: Self) -> Self {
                    self - step
                }

                #[inline]
                fn wheel(self, step: Self, wheel: f32) -> Self {
                    self + step * wheel as $t
                }

                #[inline]
                fn clamp_to(self, range: [Self; 2]) -> Self {
                    self.clamp(range[0], range[1])
                }

                #[inline]
                fn parse(text: &str) -> Option<Self> {
                    text.replace(',', ".")
                        .parse::<$t>()
                        .ok()
                        .filter(|v| v.is_finite())
                }

                #[inline]
                fn format(self, decimals: u32) -> String {
                    format!("{:.1$}", self, decimals as usize)
                }
            }
        )*
    };
}

impl_wheel_box_value_f!(f64, f32);

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(q: bool, w: bool, r: bool) -> WheelsBoxKeys {
        WheelsBoxKeys {
            q,
            w,
            r,
            ..Default::default()
        }
    }

    // Fit affixes ------------------------------------------------------------
    /// Runs `fit_affixes` for "x: " + `1234567` + "nm". `inner_width` gets the widths of
    /// (prefix, value, suffix) and returns the inner width (without margin) to test.
    /// Returns whether the (prefix, suffix) are shown.
    fn fit(inner_width: impl Fn(f32, f32, f32) -> f32) -> (bool, bool) {
        let value = "1234567";
        let mut result = None;
        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            let prefix = Atoms::new("x: ");
            let suffix = Atoms::new("nm");
            let inner = inner_width(
                affix_width(ui, &prefix),
                atom_width(ui, RichText::new(value).into()),
                affix_width(ui, &suffix),
            );
            let affixes = fit_affixes(ui, inner + MARGIN.sum().x, prefix, value, suffix);
            result = Some((!affixes.prefix.is_empty(), !affixes.suffix.is_empty()));
        });
        harness.run();
        drop(harness);
        result.unwrap()
    }

    #[test]
    fn fit_affixes_all_fit() {
        assert_eq!(fit(|p, v, s| p + v + s), (true, true));
    }

    #[test]
    fn fit_affixes_hides_suffix_first() {
        assert_eq!(fit(|p, v, s| p + v + s - 1.), (true, false));
        assert_eq!(fit(|p, v, _| p + v), (true, false));
    }

    #[test]
    fn fit_affixes_then_hides_prefix() {
        assert_eq!(fit(|p, v, _| p + v - 1.), (false, false));
        // the value itself doesn't fit and will be truncated
        assert_eq!(fit(|_, v, _| v / 2.), (false, false));
    }

    #[test]
    fn same_treats_nan_as_equal() {
        assert!(same(1.0, 1.0));
        assert!(same(f64::NAN, f64::NAN));
        assert!(!same(f64::NAN, 1.0));
        assert!(!same(1.0, 2.0));
        assert!(same(3, 3));
    }

    // Step ------------------------------------------------------------------
    #[test]
    fn int_div2_never_goes_below_one() {
        assert_eq!(8_i32.div2(), 4);
        assert_eq!(3_i32.div2(), 1);
        assert_eq!(1_i32.div2(), 1);
        assert_eq!(1_u8.div2(), 1);
    }

    #[test]
    fn int_mul2_saturates() {
        assert_eq!(4_i32.mul2(), 8);
        assert_eq!(100_u8.mul2(), 200);
        assert_eq!(200_u8.mul2(), u8::MAX);
        assert_eq!(100_i8.mul2(), i8::MAX);
        assert_eq!(i64::MAX.mul2(), i64::MAX);
    }

    #[test]
    fn float_div2_mul2() {
        assert_eq!(1.0_f64.div2(), 0.5);
        assert_eq!(0.5_f32.mul2(), 1.0);
    }

    #[test]
    fn keys_update_step() {
        assert_eq!(keys(false, false, false).update_step(4, 1), 4);
        assert_eq!(keys(true, false, false).update_step(4, 1), 2);
        assert_eq!(keys(false, true, false).update_step(4, 1), 8);
        assert_eq!(keys(false, false, true).update_step(4, 1), 1);
        // reset wins over halving / doubling pressed in the same frame
        assert_eq!(keys(true, true, true).update_step(16, 3), 3);
        assert_eq!(keys(true, false, false).update_step(0.1_f64, 1.0), 0.05);
    }

    // Format ----------------------------------------------------------------
    #[test]
    fn int_format_ignores_decimals() {
        assert_eq!(42_i32.format(0), "42");
        assert_eq!(42_i32.format(3), "42");
        assert_eq!(i32::DEFAULT_DECIMALS, 0);
    }

    #[test]
    fn float_format_uses_decimals() {
        assert_eq!(1.5_f64.format(0), "2");
        assert_eq!(1.5_f64.format(2), "1.50");
        assert_eq!(0.1_f32.format(f32::DEFAULT_DECIMALS), "0.100");
        assert_eq!(f64::DEFAULT_DECIMALS, 3);
    }

    // Integer ---------------------------------------------------------------
    #[test]
    fn parse_int_valid() {
        assert_eq!(parse_value::<i32>("42", None), Some(42));
        assert_eq!(parse_value::<i32>("-7", None), Some(-7));
        assert_eq!(parse_value::<i32>("  5 ", None), Some(5));
    }

    #[test]
    fn parse_int_invalid() {
        assert_eq!(parse_value::<i32>("", None), None);
        assert_eq!(parse_value::<i32>("abc", None), None);
        assert_eq!(parse_value::<i32>("1.5", None), None);
        assert_eq!(parse_value::<i32>("1,5", None), None);
        assert_eq!(parse_value::<u8>("-1", None), None);
        assert_eq!(parse_value::<u8>("256", None), None);
    }

    #[test]
    fn parse_int_clamps_to_range() {
        assert_eq!(parse_value("50", Some([0, 10])), Some(10));
        assert_eq!(parse_value("-50", Some([0, 10])), Some(0));
        assert_eq!(parse_value("5", Some([0, 10])), Some(5));
    }

    #[test]
    fn step_int_up_down() {
        assert_eq!(step_value(5, 2, true, None), 7);
        assert_eq!(step_value(5, 2, false, None), 3);
    }

    #[test]
    fn step_int_saturates() {
        assert_eq!(step_value(u8::MAX, 10, true, None), u8::MAX);
        assert_eq!(step_value(0_u8, 10, false, None), 0);
        assert_eq!(step_value(i32::MIN, 1, false, None), i32::MIN);
    }

    #[test]
    fn step_int_clamps_to_range() {
        assert_eq!(step_value(9, 5, true, Some([0, 10])), 10);
        assert_eq!(step_value(1, 5, false, Some([0, 10])), 0);
        // a value outside the range is pulled back in from either direction
        assert_eq!(step_value(-20, 1, true, Some([0, 10])), 0);
        assert_eq!(step_value(20, 1, false, Some([0, 10])), 10);
    }

    #[test]
    fn wheel_int_uses_direction_only() {
        assert_eq!(wheel_value(5, 2, 1.0, None), 7);
        assert_eq!(wheel_value(5, 2, 0.3, None), 7);
        assert_eq!(wheel_value(5, 2, -0.3, None), 3);
        assert_eq!(wheel_value(5, 2, 0.0, None), 5);
        assert_eq!(wheel_value(9, 5, 1.0, Some([0, 10])), 10);
    }

    // Float -----------------------------------------------------------------
    #[test]
    fn parse_float_valid() {
        assert_eq!(parse_value::<f64>("1.5", None), Some(1.5));
        assert_eq!(parse_value::<f64>("-2", None), Some(-2.0));
        assert_eq!(parse_value::<f64>("1e3", None), Some(1000.0));
        assert_eq!(parse_value::<f32>(" 0.25 ", None), Some(0.25));
    }

    #[test]
    fn parse_float_accepts_comma() {
        assert_eq!(parse_value::<f64>("1,5", None), Some(1.5));
    }

    #[test]
    fn parse_float_rejects_non_finite() {
        assert_eq!(parse_value::<f64>("NaN", None), None);
        assert_eq!(parse_value::<f64>("nan", Some([0.0, 1.0])), None);
        assert_eq!(parse_value::<f64>("inf", None), None);
        assert_eq!(parse_value::<f32>("-infinity", None), None);
    }

    #[test]
    fn parse_float_invalid() {
        assert_eq!(parse_value::<f64>("", None), None);
        assert_eq!(parse_value::<f64>("abc", None), None);
        assert_eq!(parse_value::<f64>("1.2.3", None), None);
    }

    #[test]
    fn parse_float_clamps_to_range() {
        assert_eq!(parse_value("5.5", Some([0.0, 1.0])), Some(1.0));
        assert_eq!(parse_value("-5.5", Some([0.0, 1.0])), Some(0.0));
    }

    #[test]
    fn step_float_up_down() {
        assert_eq!(step_value(1.0, 0.5, true, None), 1.5);
        assert_eq!(step_value(1.0, 0.5, false, None), 0.5);
    }

    #[test]
    fn step_float_clamps_to_range() {
        assert_eq!(step_value(0.9, 0.5, true, Some([0.0, 1.0])), 1.0);
        assert_eq!(step_value(0.1, 0.5, false, Some([0.0, 1.0])), 0.0);
        assert_eq!(step_value(-5.0, 0.5, true, Some([0.0, 1.0])), 0.0);
    }

    #[test]
    fn wheel_float_scales_by_delta() {
        assert_eq!(wheel_value(1.0, 1.0, 1.0, None), 2.0);
        assert_eq!(wheel_value(1.0, 1.0, -0.5, None), 0.5);
        assert_eq!(wheel_value(1.0, 1.0, 0.0, None), 1.0);
        assert_eq!(wheel_value(0.8, 1.0, 1.0, Some([0.0, 1.0])), 1.0);
    }
}
