use std::fmt::Display;
use std::str::FromStr;

use egui::{
    Align, Align2, AsIdSalt, Atom, AtomExt as _, AtomLayout, Atoms, Color32, CursorIcon, Event,
    FontId, FontSelection, Frame, Id, IdSalt, InputOptions, IntoAtoms, Key, Margin, Modifiers,
    MouseWheelUnit, Response, RichText, Sense, TextWrapMode, Ui, Vec2, Widget, WidgetInfo,
    response::Flags, text, text_edit, vec2,
};

/// Wheel movement in points that makes one step, about one notch of a mouse wheel.
/// Touchpads report many small movements, which add up to steps at a steady pace.
const POINTS_PER_STEP: f32 = 40.0;

// WheelBoxValue --------------------------------------------------------------
/// A number that can be edited with [`WheelBox`].
///
/// Implemented for all primitive integers and for `f32` / `f64`. Implement it for your own
/// number types (e.g. a fixed-point newtype) to edit them with [`WheelBox`].
pub trait WheelBoxValue: Copy + PartialEq + Send + Sync + 'static {
    /// Decimals shown when [`WheelBox::decimals`] is not set. Ignored by integers.
    const DEFAULT_DECIMALS: u32;

    /// The step used when [`WheelBox::default_single_step`] is not set.
    fn one() -> Self;
    /// Halve the step (Q). Must not reach zero: integer steps stay at least 1,
    /// float steps stay normal (not zero or subnormal).
    #[must_use]
    fn div2(self) -> Self;
    /// Double the step (W). Integers saturate, floats stay finite.
    #[must_use]
    fn mul2(self) -> Self;
    /// `self + step`. Integers saturate, floats stay finite.
    #[must_use]
    fn step_up(self, step: Self) -> Self;
    /// `self - step`. Integers saturate, floats stay finite.
    #[must_use]
    fn step_down(self, step: Self) -> Self;
    /// Clamp to `range`, given in either order. Must not panic, e.g. on a NaN bound.
    #[must_use]
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

/// `==` that also treats NaN as equal to itself, so a NaN set from outside is not
/// taken for a new change on every frame.
#[allow(clippy::eq_op)]
fn same<T: PartialEq + Copy>(a: T, b: T) -> bool {
    a == b || (a != a && b != b)
}

/// `sum` (of `a` and `b`) rounded to as many decimals as `a` and `b` have, so stepping
/// `0.5` by `0.1` gives `0.6` and not `0.6000000000000001`, and stepping back gives `0.0`.
///
/// Rust prints floats in their shortest round-trip form, without an exponent, so the
/// decimals can be counted from the printed text.
fn round_sum<F: Copy + Display + FromStr>(a: F, b: F, sum: F) -> F {
    fn decimals(text: &str) -> usize {
        text.split_once('.')
            .map_or(0, |(_, decimals)| decimals.len())
    }

    let decimals = decimals(&a.to_string()).max(decimals(&b.to_string()));
    format!("{sum:.decimals$}").parse().unwrap_or(sum)
}

// State ----------------------------------------------------------------------
#[derive(Clone)]
struct WheelBoxState<T> {
    /// The bound value as last seen or written, to detect changes from outside.
    value: T,
    /// The value being edited. Ahead of `value` after stepping with
    /// [`WheelBox::always_update`] off.
    state_value: T,
    /// The text of the focused box. Set from `state_value` on each focused frame unless
    /// `edited`, so it may be stale while unfocused.
    string: String,
    /// `string` was typed into since it was last set from `state_value`.
    edited: bool,
    /// The pass in which the widget was last shown focused.
    edit_pass: Option<u64>,
    single_step: T,
    /// Wheel movement in steps that hasn't made a whole step yet.
    wheel: f32,
}

impl<T: WheelBoxValue> WheelBoxState<T> {
    fn new(value: T, single_step: T) -> Self {
        Self {
            value,
            state_value: value,
            string: String::new(),
            edited: false,
            edit_pass: None,
            single_step,
            wheel: 0.0,
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
    fn sync(&mut self, value: T) {
        self.value = value;
        self.state_value = value;
    }

    /// The number to step from: the typed one if there is a valid one, otherwise the
    /// value being edited.
    fn step_start(&self, range: Option<[T; 2]>) -> T {
        if self.edited
            && let Some(v) = parse_value(&self.string, range)
        {
            return v;
        }
        self.state_value
    }

    /// The value to write when editing ends: the typed number, or a step not written
    /// yet (with [`WheelBox::always_update`] off), both clamped to the current `range`.
    /// `None` if there is nothing to write or the typed text is not a number.
    fn pending(&self, range: Option<[T; 2]>) -> Option<T> {
        if self.edited {
            parse_value(&self.string, range)
        } else if !same(self.state_value, self.value) {
            Some(clamp_value(self.state_value, range))
        } else {
            None
        }
    }

    /// Finish editing and write the pending value to `value`. Invalid text is dropped.
    /// Returns whether `value` changed.
    fn commit(&mut self, value: &mut T, range: Option<[T; 2]>, decimals: u32) -> bool {
        let changed = match self.pending(range) {
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

// Input ----------------------------------------------------------------------
/// Input handled by a focused [`WheelBox`].
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    /// Arrow up (`true`) or down.
    Step(bool),
    /// Q
    HalveStep,
    /// W
    DoubleStep,
    /// R
    ResetStep,
    /// Vertical mouse wheel movement in steps, within `[-1, 1]`.
    Wheel(f32),
}

impl Action {
    /// The step after Q (halve), W (double) or R (reset to `default`).
    fn update_step<T: WheelBoxValue>(self, step: T, default: T) -> T {
        match self {
            Self::HalveStep => step.div2(),
            Self::DoubleStep => step.mul2(),
            Self::ResetStep => default,
            Self::Step(_) | Self::Wheel(_) => step,
        }
    }
}

/// Take this frame's step shortcuts and wheel movements, in the order they happened.
///
/// The shortcut keys are consumed, so the text field doesn't see them. They match only
/// without modifiers, so e.g. Shift+Up still selects text. The letters Q, W and R are
/// never typed, in either case. Wheel events stay in the input for others to see.
fn take_actions(ui: &mut Ui) -> Vec<Action> {
    let options = ui.ctx().options(|o| o.input_options);
    ui.input_mut(|input| {
        let mut actions = Vec::new();
        input.events.retain_mut(|event| match event {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if modifiers.is_none() => {
                let action = match key {
                    Key::ArrowUp => Action::Step(true),
                    Key::ArrowDown => Action::Step(false),
                    Key::Q => Action::HalveStep,
                    Key::W => Action::DoubleStep,
                    Key::R => Action::ResetStep,
                    _ => return true,
                };
                actions.push(action);
                false
            }
            Event::Text(text) => {
                text.retain(|c| !matches!(c.to_ascii_lowercase(), 'q' | 'w' | 'r'));
                !text.is_empty()
            }
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } => {
                if let Some(steps) = wheel_steps(*unit, *delta, *modifiers, &options) {
                    actions.push(Action::Wheel(steps));
                }
                true
            }
            _ => true,
        });
        actions
    })
}

/// The vertical movement of a wheel event in steps, within `[-1, 1]` so that one event
/// never makes more than one step. `None` if there is no vertical movement, or egui
/// uses the event to zoom or to scroll horizontally.
fn wheel_steps(
    unit: MouseWheelUnit,
    delta: Vec2,
    modifiers: Modifiers,
    options: &InputOptions,
) -> Option<f32> {
    if modifiers.matches_any(options.zoom_modifier) {
        return None;
    }
    let horizontal = modifiers.matches_any(options.horizontal_scroll_modifier);
    let vertical = modifiers.matches_any(options.vertical_scroll_modifier);
    let dy = match (horizontal, vertical) {
        (true, false) => return None,
        (false, true) => delta.x + delta.y,
        _ => delta.y,
    };
    let steps = match unit {
        MouseWheelUnit::Point => dy / POINTS_PER_STEP,
        MouseWheelUnit::Line | MouseWheelUnit::Page => dy,
    };
    (steps != 0.0).then(|| steps.clamp(-1.0, 1.0))
}

/// Add wheel movement in `steps` to the movement `accumulated` so far and take the
/// whole steps out of it. Turning the wheel the other way starts over.
#[allow(clippy::cast_possible_truncation)]
fn accumulate_wheel(accumulated: &mut f32, steps: f32) -> i32 {
    if accumulated.signum() != steps.signum() {
        *accumulated = 0.0;
    }
    *accumulated += steps;
    // A little slack, so ten movements of 0.1 make a step despite rounding.
    let whole = (*accumulated + accumulated.signum() * 1e-4).trunc();
    *accumulated -= whole;
    whole as i32
}

// WheelBox -------------------------------------------------------------------
/// Numeric input box for integers and floats.
///
/// Click (or Tab) to focus, then step the value with the arrow keys or the mouse wheel,
/// change the step with Q (halve), W (double) and R (reset), or type a number and press
/// Enter. Typed text is also committed when the box loses focus, and discarded on Escape.
/// The text is selected on focus and after each step, so typing replaces it.
///
/// The value is kept at full precision, [`WheelBox::decimals`] only limits what is
/// shown. [`Response::changed`] is set only when the value changed.
///
/// Like [`egui::DragValue`], it follows the [`egui::Style`] of the [`Ui`]: the size comes
/// from `spacing.button_padding` and `spacing.interact_size`, the font from
/// `drag_value_text_style`, and the colors from `visuals.widgets` (and the text field
/// colors while editing).
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// # use egui_widgets::WheelBox;
/// # let (mut count, mut exposure) = (1_u32, 1.0_f64);
/// ui.add(WheelBox::new(&mut count).range([0, 100]).prefix("n: "));
/// ui.add(WheelBox::new(&mut exposure).decimals(2).suffix(egui::RichText::new("ms").weak()));
/// # });
/// ```
#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct WheelBox<'a, T> {
    id_salt: Option<IdSalt>,
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
    /// Edit `value`.
    pub fn new(value: &'a mut T) -> Self {
        Self {
            id_salt: None,
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

    /// Use a stable id instead of one based on the position in the [`Ui`], so the box
    /// keeps its focus and edit when widgets before it come and go.
    #[inline]
    pub fn id_salt(mut self, id_salt: impl AsIdSalt) -> Self {
        self.id_salt = Some(IdSalt::new(id_salt));
        self
    }

    /// Number of decimals shown for floats (default 3). Ignored by integers.
    #[inline]
    pub fn decimals(mut self, decimals: u32) -> Self {
        self.decimals = Some(decimals);
        self
    }

    /// Clamp typed and stepped values to `range`, given in either order.
    ///
    /// A value set from outside (by the application) is shown as it is, and clamped only
    /// once the user changes it.
    #[inline]
    pub fn range(mut self, range: [T; 2]) -> Self {
        self.range = Some(range);
        self
    }

    /// Color of the value. Prefix and suffix keep their own color.
    #[inline]
    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Shown before the value, also while editing, e.g. a label `"x: "` or an icon.
    #[inline]
    pub fn prefix(mut self, prefix: impl IntoAtoms<'static>) -> Self {
        self.prefix = prefix.into_atoms();
        self
    }

    /// Shown at the right edge, also while editing, e.g. a unit `"ms"`
    /// or `RichText::new("ms").weak()`.
    ///
    /// If the box is too narrow, the suffix is hidden first, then the prefix, and only
    /// then is the value truncated.
    #[inline]
    pub fn suffix(mut self, suffix: impl IntoAtoms<'static>) -> Self {
        self.suffix = suffix.into_atoms();
        self
    }

    /// Width of the box, including prefix and suffix. Defaults to the available width.
    #[inline]
    pub fn desired_width(mut self, width: f32) -> Self {
        self.desired_width = Some(width);
        self
    }

    /// Write stepped values right away (default). If off, the value is written when
    /// editing ends, like typed text.
    #[inline]
    pub fn always_update(mut self, always_update: bool) -> Self {
        self.always_update = always_update;
        self
    }

    /// The step to start with and to return to with R (default 1). Should be positive.
    #[inline]
    pub fn default_single_step(mut self, default_single_step: T) -> Self {
        self.default_single_step = Some(default_single_step);
        self
    }

    /// Keep the step in `single_step` instead of in the widget, so the application can
    /// read or change it, or share it between boxes. Q, W and R update it. Should be
    /// positive.
    #[inline]
    pub fn single_step(mut self, single_step: &'a mut T) -> Self {
        self.single_step = Some(single_step);
        self
    }
}

impl<T: WheelBoxValue> Widget for WheelBox<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let id = self
            .id_salt
            .map_or_else(|| ui.next_auto_id(), |salt| ui.make_persistent_id(salt));
        // Not clamped to the available width: in a `Grid` that would pin the column
        // to its initial width. The `TextEdit` is forced to the same width via `min_size`.
        let width = self.desired_width.unwrap_or_else(|| ui.available_width());
        let geometry = Geometry::new(ui, width);
        let decimals = self.decimals.unwrap_or(T::DEFAULT_DECIMALS);
        let default_single_step = self.default_single_step.unwrap_or_else(T::one);

        let has_focus = ui.is_enabled() && ui.memory(|m| m.has_focus(id));
        let mut state = ui.data(|d| d.get_temp::<WheelBoxState<T>>(id));

        // Changed from outside, e.g. by the application or another widget showing
        // the same value.
        if let Some(state) = &mut state
            && !same(state.value, *self.value)
        {
            state.sync(*self.value);
        }

        if has_focus {
            let state =
                state.unwrap_or_else(|| WheelBoxState::new(*self.value, default_single_step));
            self.ui_focused(ui, id, &geometry, decimals, state)
        } else {
            self.ui_unfocused(ui, id, &geometry, decimals, state)
        }
    }
}

impl<T: WheelBoxValue> WheelBox<'_, T> {
    fn ui_focused(
        self,
        ui: &mut Ui,
        id: Id,
        geometry: &Geometry,
        decimals: u32,
        mut state: WheelBoxState<T>,
    ) -> Response {
        let Self {
            default_single_step,
            always_update,
            range,
            prefix,
            suffix,
            text_color,
            value,
            single_step,
            ..
        } = self;
        let default_single_step = default_single_step.unwrap_or_else(T::one);
        let pass = ui.ctx().cumulative_pass_nr();

        // Follow a change of the value or of `decimals`, unless the text was typed into.
        let value_text = state.state_value.format(decimals);
        if !state.edited {
            state.string.clone_from(&value_text);
        }

        // Just focused, by a click, Tab or a focus request.
        if state.edit_pass.is_none_or(|last| pass > last + 1) {
            state.wheel = 0.0;
            select_all(ui, id, &state.string);
        }

        let actions = take_actions(ui);

        // Fit to the value, not to the text being typed, so the prefix and suffix
        // don't come and go while typing.
        let affixes = fit_affixes(ui, geometry, prefix, &value_text, text_color, suffix);
        let mut response = text_edit::TextEdit::singleline(&mut state.string)
            .font(geometry.font_id.clone())
            .desired_width(geometry.size.x)
            .min_size(geometry.size)
            .vertical_align(Align::Center)
            .clip_text(true)
            .margin(geometry.margin)
            .prefix(affixes.prefix)
            .suffix(affixes.suffix)
            .text_color_opt(text_color)
            .id(id)
            .ui(ui);
        state.edited |= response.changed();
        // `changed()` reports changes of the value, not of the text being typed.
        response.flags.remove(Flags::CHANGED);

        let hovered = response.hovered();
        if hovered {
            // Vertical scrolling over the focused box steps it, so it must not also
            // scroll an enclosing `ScrollArea`.
            ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }

        let mut step = single_step.as_deref().copied().unwrap_or(state.single_step);
        let mut stepped = None;
        for action in actions {
            let steps = match action {
                Action::Step(up) => i32::from(up) * 2 - 1,
                Action::Wheel(steps) if hovered => accumulate_wheel(&mut state.wheel, steps),
                Action::Wheel(_) => 0,
                Action::HalveStep | Action::DoubleStep | Action::ResetStep => {
                    step = action.update_step(step, default_single_step);
                    0
                }
            };
            for _ in 0..steps.unsigned_abs() {
                let start = stepped.unwrap_or_else(|| state.step_start(range));
                stepped = Some(step_value(start, step, steps > 0, range));
            }
        }
        match single_step {
            Some(single_step) => *single_step = step,
            None => state.single_step = step,
        }

        if let Some(v) = stepped {
            state.set_value(v, decimals);
            if always_update && !same(v, *value) {
                *value = v;
                state.value = v;
                response.mark_changed();
            }
            // So typing replaces the stepped value.
            select_all(ui, id, &state.string);
        }

        if response.lost_focus() {
            if state.commit(value, range, decimals) {
                response.mark_changed();
            }
            text_edit::TextEditState::default().store(ui.ctx(), id);
        } else {
            state.edit_pass = Some(pass);
        }

        ui.data_mut(|d| d.insert_temp(id, state));
        response
    }

    fn ui_unfocused(
        self,
        ui: &mut Ui,
        id: Id,
        geometry: &Geometry,
        decimals: u32,
        state: Option<WheelBoxState<T>>,
    ) -> Response {
        let Self {
            range,
            prefix,
            suffix,
            text_color,
            value,
            ..
        } = self;
        let mut changed = false;

        // A box that was never focused has no state and just shows the value.
        if let Some(mut state) = state {
            let mut save_state = false;

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
            if !same(state.state_value, *value) || state.edited {
                state.reset(*value, decimals);
                save_state = true;
            }

            if save_state {
                ui.data_mut(|d| d.insert_temp(id, state));
            }
        }

        let text = value.format(decimals);
        // The same color as the focused `TextEdit`, so the text doesn't change on focus.
        let text_color = text_color
            .or(ui.visuals().override_text_color)
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
        let affixes = fit_affixes(ui, geometry, prefix, &text, Some(text_color), suffix);
        let mut response = show_unfocused(ui, id, geometry, affixes, &text, text_color);
        if changed {
            response.mark_changed();
        }
        response
    }
}

// Helpers -------------------------------------------------------------------
/// Font, inner margin and size shared by the focused [`text_edit::TextEdit`] and the
/// unfocused layout, so the widget keeps its size and the text doesn't move when it gains
/// focus. Taken from the style like in [`egui::DragValue`], so the box lines up with
/// buttons and other widgets in a row.
struct Geometry {
    font_id: FontId,
    margin: Margin,
    size: Vec2,
}

impl Geometry {
    fn new(ui: &Ui, width: f32) -> Self {
        let style = ui.style();
        let font_id = FontSelection::Style(style.drag_value_text_style.clone()).resolve(style);
        let margin = Margin::from(style.spacing.button_padding);
        let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
        let height = (row_height + style.spacing.extra_text_line_spacing + margin.sum().y)
            .max(style.spacing.interact_size.y);
        Self {
            font_id,
            margin,
            size: vec2(width, height),
        }
    }

    /// Width left for the prefix, value and suffix.
    fn inner_width(&self) -> f32 {
        self.size.x - self.margin.sum().x
    }
}

/// Select all text of the focused text field with `id`, so typing replaces it.
fn select_all(ui: &Ui, id: Id, text: &str) {
    let mut state = text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
    state.cursor.set_char_range(Some(text::CCursorRange::two(
        text::CCursor::default(),
        text::CCursor::new(text.chars().count()),
    )));
    state.store(ui.ctx(), id);
}

/// Prefix and suffix that fit next to the value.
struct Affixes {
    prefix: Atoms<'static>,
    suffix: Atoms<'static>,
    /// The value was measured and fits next to them, so it isn't truncated.
    value_fits: bool,
}

/// Hide the suffix, then the prefix, if they don't fit next to the value in the box,
/// so the value is truncated only when it doesn't fit on its own.
fn fit_affixes(
    ui: &Ui,
    geometry: &Geometry,
    prefix: Atoms<'static>,
    value_text: &str,
    text_color: Option<Color32>,
    suffix: Atoms<'static>,
) -> Affixes {
    // Nothing to hide, so skip laying out the value just to measure it.
    if prefix.is_empty() && suffix.is_empty() {
        return Affixes {
            prefix,
            suffix,
            value_fits: false,
        };
    }

    let available = geometry.inner_width();
    let value = value_rich_text(geometry, value_text, text_color);
    let width = affix_width(ui, &prefix) + atom_width(ui, value.into());
    if width <= available {
        // The suffix is measured only if it can be shown.
        let suffix = if width + affix_width(ui, &suffix) <= available {
            suffix
        } else {
            Atoms::default()
        };
        Affixes {
            prefix,
            suffix,
            value_fits: true,
        }
    } else {
        Affixes {
            prefix: Atoms::default(),
            suffix: Atoms::default(),
            value_fits: false,
        }
    }
}

/// The value as shown, in the font of the focused [`text_edit::TextEdit`]. Measured and
/// shown with the same color, so both use the same cached galley.
fn value_rich_text(geometry: &Geometry, value_text: &str, color: Option<Color32>) -> RichText {
    let text = RichText::new(value_text).font(geometry.font_id.clone());
    match color {
        Some(color) => text.color(color),
        None => text,
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
#[allow(clippy::cast_possible_truncation)]
fn show_unfocused(
    ui: &mut Ui,
    id: Id,
    geometry: &Geometry,
    affixes: Affixes,
    value_text: &str,
    text_color: Color32,
) -> Response {
    let Affixes {
        prefix,
        suffix,
        value_fits,
    } = affixes;

    // The value takes the free space (like the text in `TextEdit`), so the suffix sits at
    // the right edge in both states. It is truncated only if it doesn't fit on its own.
    let mut atoms = prefix;
    atoms.push_right(
        value_rich_text(geometry, value_text, Some(text_color))
            .atom_grow(true)
            .atom_shrink(true)
            .atom_align(Align2::LEFT_CENTER),
    );
    atoms.extend_right(suffix);

    // Click only, not drag: on touch screens a drag starting on the box should scroll an
    // enclosing `ScrollArea`, not focus the box.
    let mut allocated = AtomLayout::new(atoms)
        .id(id)
        .sense(Sense::click())
        .frame(Frame::new().inner_margin(geometry.margin))
        .min_size(geometry.size)
        .max_width(geometry.size.x)
        .align2(Align2::LEFT_CENTER)
        // Unwrapped when it fits, as when it was measured, so the galley isn't laid out
        // again.
        .wrap_mode(if value_fits {
            TextWrapMode::Extend
        } else {
            TextWrapMode::Truncate
        })
        .allocate(ui);

    // Same frame adjustments as `TextEdit`, with the regular widget colors.
    let visuals = ui.style().interact(&allocated.response);
    allocated.frame = allocated
        .frame
        .fill(visuals.bg_fill)
        .corner_radius(visuals.corner_radius)
        .stroke(visuals.bg_stroke)
        .inner_margin(
            geometry.margin
                + Margin::same((visuals.expansion - visuals.bg_stroke.width).round() as i8),
        )
        .outer_margin(Margin::same(-(visuals.expansion as i8)));

    let mut response = allocated
        .paint(ui)
        .response
        .on_hover_cursor(CursorIcon::Text);
    if ui.style().explanation_tooltips {
        response = response.on_hover_text(
            "Click to edit. Step with the arrow keys or the mouse wheel, \
             change the step with Q (halve), W (double) and R (reset).",
        );
    }

    if response.clicked() {
        response.request_focus();
    }

    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::text_edit(enabled, value_text, value_text, ""));

    response
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
                    (self / 2).max(1)
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
                fn clamp_to(self, [a, b]: [Self; 2]) -> Self {
                    if b < a { self.clamp(b, a) } else { self.clamp(a, b) }
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

impl_wheel_box_value!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

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
                    let v = self / 2.;
                    if v.is_normal() { v } else { self }
                }

                #[inline]
                fn mul2(self) -> Self {
                    let v = self * 2.;
                    if v.is_finite() { v } else { self }
                }

                #[inline]
                fn step_up(self, step: Self) -> Self {
                    let sum = self + step;
                    if sum.is_infinite() {
                        return if sum > 0. { <$t>::MAX } else { <$t>::MIN };
                    }
                    // `+ 0.` turns `-0.0` into `0.0`, which is shown without a sign.
                    round_sum(self, step, sum) + 0.
                }

                #[inline]
                fn step_down(self, step: Self) -> Self {
                    self.step_up(-step)
                }

                #[inline]
                fn clamp_to(self, [a, b]: [Self; 2]) -> Self {
                    let (lo, hi) = if b < a { (b, a) } else { (a, b) };
                    // Unlike `clamp`, `max` and `min` ignore a NaN bound instead of panicking.
                    self.max(lo).min(hi)
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

impl_wheel_box_value_f!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut geometry = Geometry::new(ui, 0.);
            let inner = inner_width(
                affix_width(ui, &prefix),
                atom_width(ui, value_rich_text(&geometry, value, None).into()),
                affix_width(ui, &suffix),
            );
            geometry.size.x = inner + geometry.margin.sum().x;
            let affixes = fit_affixes(ui, &geometry, prefix, value, None, suffix);
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
    fn float_step_stays_normal_and_finite() {
        assert_eq!(f32::MIN_POSITIVE.div2(), f32::MIN_POSITIVE);
        assert_eq!(f64::MIN_POSITIVE.div2(), f64::MIN_POSITIVE);
        assert_eq!(f32::MAX.mul2(), f32::MAX);
        assert_eq!(f64::MAX.mul2(), f64::MAX);
    }

    #[test]
    fn update_step() {
        assert_eq!(Action::Step(true).update_step(4, 1), 4);
        assert_eq!(Action::HalveStep.update_step(4, 1), 2);
        assert_eq!(Action::DoubleStep.update_step(4, 1), 8);
        assert_eq!(Action::ResetStep.update_step(4, 1), 1);
        assert_eq!(Action::HalveStep.update_step(0.1_f64, 1.0), 0.05);
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
    fn wide_and_pointer_sized_ints() {
        assert_eq!(
            parse_value::<i128>("-170141183460469231731687303715884105728", None),
            Some(i128::MIN)
        );
        assert_eq!(u128::MAX.step_up(1), u128::MAX);
        assert_eq!(parse_value::<usize>("7", Some([0, 5])), Some(5));
        assert_eq!((-3_isize).format(2), "-3");
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
    fn reversed_range_is_accepted() {
        assert_eq!(15.clamp_to([10, 0]), 10);
        assert_eq!((-5).clamp_to([10, 0]), 0);
        assert_eq!(1.5_f64.clamp_to([1.0, 0.0]), 1.0);
        assert_eq!(step_value(9, 5, true, Some([10, 0])), 10);
    }

    #[test]
    fn nan_range_bound_is_ignored() {
        assert_eq!(5.0_f64.clamp_to([0.0, f64::NAN]), 5.0);
        assert_eq!((-5.0_f64).clamp_to([0.0, f64::NAN]), 0.0);
        assert_eq!(5.0_f64.clamp_to([f64::NAN, 1.0]), 1.0);
        assert_eq!(5.0_f32.clamp_to([f32::NAN, f32::NAN]), 5.0);
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
        assert_eq!(parse_value::<f64>("1e400", None), None);
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
    fn step_float_does_not_drift() {
        let mut v = 0.5_f64;
        for expected in [0.6, 0.7, 0.8, 0.9, 1.0] {
            v = v.step_up(0.1);
            assert_eq!(v, expected);
        }
        for _ in 0..10 {
            v = v.step_down(0.1);
        }
        assert_eq!(v, 0.0);
        assert!(v.is_sign_positive(), "shown as 0.000, not -0.000");
        assert_eq!(0.1_f32.step_up(0.2), 0.3);
    }

    #[test]
    fn step_float_keeps_full_precision() {
        assert_eq!(1.23456_f64.step_up(0.1), 1.33456);
        assert_eq!(1.0_f64.step_up(0.001), 1.001);
        assert_eq!(1e-300_f64.step_up(1e-300), 2e-300);
        assert_eq!(1e300_f64.step_up(1e300), 2e300);
    }

    #[test]
    fn step_float_saturates() {
        assert_eq!(f64::MAX.step_up(f64::MAX), f64::MAX);
        assert_eq!(f64::MIN.step_down(f64::MAX), f64::MIN);
        assert_eq!(f32::MAX.step_up(f32::MAX), f32::MAX);
    }

    // Wheel -----------------------------------------------------------------
    fn steps(unit: MouseWheelUnit, delta: Vec2, modifiers: Modifiers) -> Option<f32> {
        wheel_steps(unit, delta, modifiers, &InputOptions::default())
    }

    #[test]
    fn wheel_steps_per_event() {
        // a mouse wheel notch is one step
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(0., 1.), Modifiers::NONE),
            Some(1.)
        );
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(0., -3.), Modifiers::NONE),
            Some(-1.)
        );
        // touchpads move in points
        assert_eq!(
            steps(MouseWheelUnit::Point, vec2(0., 10.), Modifiers::NONE),
            Some(0.25)
        );
        assert_eq!(
            steps(MouseWheelUnit::Point, vec2(0., 100.), Modifiers::NONE),
            Some(1.)
        );
    }

    #[test]
    fn wheel_steps_ignore_zoom_and_horizontal_scrolling() {
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(1., 0.), Modifiers::NONE),
            None
        );
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(0., 1.), Modifiers::SHIFT),
            None
        );
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(0., 1.), Modifiers::COMMAND),
            None
        );
        // the vertical scroll modifier turns horizontal movement into vertical
        assert_eq!(
            steps(MouseWheelUnit::Line, vec2(1., 0.), Modifiers::ALT),
            Some(1.)
        );
    }

    #[test]
    fn wheel_accumulates_to_whole_steps() {
        let mut acc = 0.0;
        assert_eq!(accumulate_wheel(&mut acc, 1.0), 1);
        assert_eq!(accumulate_wheel(&mut acc, 0.5), 0);
        assert_eq!(accumulate_wheel(&mut acc, 0.5), 1);
        let ten_tenths: i32 = (0..10).map(|_| accumulate_wheel(&mut acc, 0.1)).sum();
        assert_eq!(ten_tenths, 1);
        // turning the other way starts over
        assert_eq!(accumulate_wheel(&mut acc, 0.5), 0);
        assert_eq!(accumulate_wheel(&mut acc, -0.5), 0);
        assert_eq!(accumulate_wheel(&mut acc, -0.5), -1);
    }
}
