use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, MouseWheelUnit, TouchPhase, vec2};
use egui_kittest::kittest::{Queryable, by};
use egui_kittest::{Harness, Node};
use egui_widgets::WheelBox;

fn int_harness(
    value: i32,
    build: fn(WheelBox<'_, i32>) -> WheelBox<'_, i32>,
) -> Harness<'static, i32> {
    let mut harness = Harness::new_ui_state(
        move |ui, value: &mut i32| {
            ui.add(build(WheelBox::new(value)));
        },
        value,
    );
    harness.run();
    harness
}

fn float_harness(
    value: f64,
    build: fn(WheelBox<'_, f64>) -> WheelBox<'_, f64>,
) -> Harness<'static, f64> {
    let mut harness = Harness::new_ui_state(
        move |ui, value: &mut f64| {
            ui.add(build(WheelBox::new(value).decimals(2)));
        },
        value,
    );
    harness.run();
    harness
}

/// The wheel box is exposed as a text input whose value is the displayed number.
fn input<'t, S>(harness: &'t Harness<'_, S>, value_text: &'t str) -> Node<'t> {
    harness.get(by().role(Role::TextInput).value(value_text))
}

fn focus(harness: &mut Harness<'_, impl Sized>, value_text: &str) {
    input(harness, value_text).click();
    harness.run();
    assert!(input(harness, value_text).is_focused());
}

fn press(harness: &mut Harness<'_, impl Sized>, key: Key) {
    harness.key_press(key);
    harness.run();
}

fn replace_text(harness: &mut Harness<'_, impl Sized>, text: &str) {
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.event(Event::Text(text.to_owned()));
    harness.run();
}

fn wheel(harness: &mut Harness<'_, impl Sized>, delta_y: f32) {
    harness.event(Event::MouseWheel {
        unit: MouseWheelUnit::Line,
        delta: vec2(0., delta_y),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

// WheelBox -------------------------------------------------------------------
#[test]
fn accesskit_value_excludes_suffix() {
    let harness = int_harness(5, |w| w.suffix("px"));
    input(&harness, "5");
}

#[test]
fn click_focuses() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");
}

#[test]
fn arrow_keys_step_value() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 6);

    press(&mut harness, Key::ArrowDown);
    press(&mut harness, Key::ArrowDown);
    assert_eq!(*harness.state(), 4);
}

#[test]
fn arrow_keys_ignored_when_not_focused() {
    let mut harness = int_harness(5, |w| w);
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 5);
}

#[test]
fn step_keys_change_step() {
    let mut harness = int_harness(0, |w| w.default_single_step(4));
    focus(&mut harness, "0");

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 4);

    // Q halves the step
    press(&mut harness, Key::Q);
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 6);

    // W doubles the step
    press(&mut harness, Key::W);
    press(&mut harness, Key::W);
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 14);

    // R resets the step to default
    press(&mut harness, Key::R);
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 18);

    // the shortcut letters are not typed into the text field
    input(&harness, "18");
}

#[test]
fn external_single_step_is_updated() {
    let mut harness = Harness::new_ui_state(
        |ui, (value, step): &mut (i32, i32)| {
            ui.add(WheelBox::new(value).single_step(step));
        },
        (0, 2),
    );
    harness.run();
    focus(&mut harness, "0");

    press(&mut harness, Key::W);
    assert_eq!(harness.state().1, 4);

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), (4, 4));
}

#[test]
fn range_is_respected() {
    let mut harness = int_harness(9, |w| w.range([0, 10]));
    focus(&mut harness, "9");

    press(&mut harness, Key::ArrowUp);
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 10);
}

#[test]
fn typing_and_enter_commits_value() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    assert_eq!(*harness.state(), 5, "value is committed only on Enter");

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 42);
    assert!(!input(&harness, "42").is_focused());
}

#[test]
fn typed_value_is_clamped_to_range() {
    let mut harness = int_harness(5, |w| w.range([0, 10]));
    focus(&mut harness, "5");

    replace_text(&mut harness, "100");
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 10);
}

#[test]
fn invalid_text_is_reverted() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "abc");
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 5);
    input(&harness, "5");
}

#[test]
fn mouse_wheel_steps_value_when_focused() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    wheel(&mut harness, 1.0);
    assert_eq!(*harness.state(), 6);

    wheel(&mut harness, -1.0);
    wheel(&mut harness, -1.0);
    assert_eq!(*harness.state(), 4);
}

#[test]
fn mouse_wheel_ignored_when_not_focused() {
    let mut harness = int_harness(5, |w| w);
    input(&harness, "5").hover();
    harness.run();

    wheel(&mut harness, 1.0);
    assert_eq!(*harness.state(), 5);
}

#[test]
fn without_always_update_value_changes_on_enter() {
    let mut harness = int_harness(5, |w| w.always_update(false));
    focus(&mut harness, "5");

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 5);
    input(&harness, "6");

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 6);
}

#[test]
fn external_value_change_is_displayed() {
    let mut harness = int_harness(5, |w| w);
    *harness.state_mut() = 7;
    harness.run();
    input(&harness, "7");
}

#[test]
fn typing_then_arrow_steps_typed_value() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 43);
    input(&harness, "43");
}

#[test]
fn typing_then_wheel_steps_typed_value() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    wheel(&mut harness, -1.0);
    assert_eq!(*harness.state(), 41);
}

#[test]
fn invalid_text_then_arrow_steps_value() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "abc");
    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 6);
    input(&harness, "6");
}

#[test]
fn escape_discards_typed_text() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    press(&mut harness, Key::Escape);
    assert_eq!(*harness.state(), 5);
    assert!(!input(&harness, "5").is_focused());
}

#[test]
fn without_always_update_escape_discards_step() {
    let mut harness = int_harness(5, |w| w.always_update(false));
    focus(&mut harness, "5");

    press(&mut harness, Key::ArrowUp);
    press(&mut harness, Key::Escape);
    assert_eq!(*harness.state(), 5);
    input(&harness, "5");
}

// External changes while focused ---------------------------------------------
#[test]
fn external_change_while_focused_is_stepped() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    *harness.state_mut() = 20;
    harness.run();
    assert!(input(&harness, "20").is_focused());

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 21);
}

#[test]
fn external_change_while_focused_is_kept_on_enter() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    *harness.state_mut() = 20;
    harness.run();
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 20);
}

#[test]
fn without_always_update_external_change_replaces_step() {
    let mut harness = int_harness(5, |w| w.always_update(false));
    focus(&mut harness, "5");

    press(&mut harness, Key::ArrowUp);
    *harness.state_mut() = 20;
    harness.run();
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 20);
}

#[test]
fn external_change_keeps_typed_text() {
    let mut harness = int_harness(5, |w| w);
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    *harness.state_mut() = 20;
    harness.run();
    input(&harness, "42");

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 42);
}

// Focus transfer ---------------------------------------------------------------
fn two_boxes(build: fn(WheelBox<'_, i32>) -> WheelBox<'_, i32>) -> Harness<'static, (i32, i32)> {
    let mut harness = Harness::new_ui_state(
        move |ui, (first, second): &mut (i32, i32)| {
            ui.add(build(WheelBox::new(first)));
            ui.add(build(WheelBox::new(second)));
        },
        (5, 10),
    );
    harness.run();
    harness
}

#[test]
fn focus_moving_down_commits_typed_text() {
    let mut harness = two_boxes(|w| w);
    focus(&mut harness, "5");
    replace_text(&mut harness, "42");

    focus(&mut harness, "10");
    assert_eq!(*harness.state(), (42, 10));
}

#[test]
fn focus_moving_up_commits_typed_text() {
    let mut harness = two_boxes(|w| w);
    focus(&mut harness, "10");
    replace_text(&mut harness, "42");

    focus(&mut harness, "5");
    assert_eq!(*harness.state(), (5, 42));
}

#[test]
fn without_always_update_focus_moving_down_commits_step() {
    let mut harness = two_boxes(|w| w.always_update(false));
    focus(&mut harness, "5");
    press(&mut harness, Key::ArrowUp);

    focus(&mut harness, "10");
    assert_eq!(*harness.state(), (6, 10));
}

#[test]
fn without_always_update_focus_moving_up_commits_step() {
    let mut harness = two_boxes(|w| w.always_update(false));
    focus(&mut harness, "10");
    press(&mut harness, Key::ArrowUp);

    focus(&mut harness, "5");
    assert_eq!(*harness.state(), (5, 11));
}

#[test]
fn hiding_while_editing_discards_typed_text() {
    let mut harness = Harness::new_ui_state(
        |ui, (value, visible): &mut (i32, bool)| {
            if *visible {
                ui.add(WheelBox::new(value));
            }
        },
        (5, true),
    );
    harness.run();
    focus(&mut harness, "5");
    replace_text(&mut harness, "42");

    harness.state_mut().1 = false;
    harness.run();
    harness.state_mut().1 = true;
    harness.run();
    assert_eq!(*harness.state(), (5, true));
    input(&harness, "5");
}

// Response::changed ------------------------------------------------------------
/// Counts the frames in which the response reported `changed()`.
fn changed_harness(value: i32) -> Harness<'static, (i32, usize)> {
    let mut harness = Harness::new_ui_state(
        |ui, (value, changed): &mut (i32, usize)| {
            if ui.add(WheelBox::new(value).range([0, 10])).changed() {
                *changed += 1;
            }
        },
        (value, 0),
    );
    harness.run();
    harness
}

#[test]
fn changed_is_not_reported_while_typing() {
    let mut harness = changed_harness(5);
    focus(&mut harness, "5");

    replace_text(&mut harness, "7");
    assert_eq!(*harness.state(), (5, 0));

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), (7, 1));
}

#[test]
fn changed_is_not_reported_without_a_change() {
    let mut harness = changed_harness(10);
    focus(&mut harness, "10");

    // at the range limit
    press(&mut harness, Key::ArrowUp);
    // untouched text
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), (10, 0));
}

// Prefix / suffix and layout -------------------------------------------------
#[test]
fn accesskit_value_excludes_prefix() {
    let harness = int_harness(5, |w| w.prefix("x: ").suffix("px"));
    input(&harness, "5");
}

#[test]
fn typing_with_prefix_and_suffix() {
    let mut harness = int_harness(5, |w| w.prefix("x: ").suffix("px"));
    focus(&mut harness, "5");

    replace_text(&mut harness, "42");
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 42);
    input(&harness, "42");
}

#[test]
fn size_is_stable_when_focused() {
    let mut harness = int_harness(5, |w| w.prefix("x: ").suffix("px").desired_width(100.));
    let unfocused = input(&harness, "5").rect();
    assert_eq!(unfocused.width(), 100.);

    focus(&mut harness, "5");
    let focused = input(&harness, "5").rect();
    assert_eq!(focused, unfocused);
}

#[test]
fn long_value_keeps_desired_width() {
    let mut harness = Harness::new_ui_state(
        |ui, value: &mut i64| {
            ui.add(WheelBox::new(value).suffix("px").desired_width(40.));
        },
        i64::MAX,
    );
    harness.run();
    let text = i64::MAX.to_string();
    assert_eq!(input(&harness, &text).rect().width(), 40.);
}

#[test]
fn desired_width_is_respected_in_grid() {
    let mut harness = Harness::new_ui_state(
        |ui, value: &mut i32| {
            egui::Grid::new("grid").show(ui, |ui| {
                ui.label("value");
                ui.add(
                    WheelBox::new(value)
                        .prefix("x: ")
                        .suffix("px")
                        .desired_width(120.),
                );
                ui.end_row();
            });
        },
        20,
    );
    harness.run();
    assert_eq!(input(&harness, "20").rect().width(), 120.);

    focus(&mut harness, "20");
    assert_eq!(input(&harness, "20").rect().width(), 120.);
}

// Floats ------------------------------------------------------------------
#[test]
fn float_default_decimals() {
    let mut harness = Harness::new_ui_state(
        |ui, value: &mut f64| {
            ui.add(WheelBox::new(value));
        },
        1.0,
    );
    harness.run();
    input(&harness, "1.000");
}

#[test]
fn int_ignores_decimals() {
    let harness = int_harness(5, |w| w.decimals(3));
    input(&harness, "5");
}

#[test]
fn float_shows_decimals() {
    let harness = float_harness(1.0, |w| w);
    input(&harness, "1.00");
}

#[test]
fn float_arrow_keys_and_step_keys() {
    let mut harness = float_harness(1.0, |w| w);
    focus(&mut harness, "1.00");

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 2.0);

    press(&mut harness, Key::Q);
    press(&mut harness, Key::ArrowDown);
    assert_eq!(*harness.state(), 1.5);
    input(&harness, "1.50");
}

#[test]
fn float_typing_accepts_comma() {
    let mut harness = float_harness(1.0, |w| w);
    focus(&mut harness, "1.00");

    replace_text(&mut harness, "2,5");
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 2.5);
    input(&harness, "2.50");
}

#[test]
fn float_rejects_nan() {
    let mut harness = float_harness(1.0, |w| w.range([0.0, 10.0]));
    focus(&mut harness, "1.00");

    replace_text(&mut harness, "nan");
    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 1.0);
}

#[test]
fn float_enter_keeps_full_precision() {
    let mut harness = float_harness(1.23456, |w| w);
    focus(&mut harness, "1.23");

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 1.23456);
}

#[test]
fn float_step_below_decimals_is_kept_on_enter() {
    let mut harness = float_harness(1.0, |w| w.default_single_step(0.001));
    focus(&mut harness, "1.00");

    press(&mut harness, Key::ArrowUp);
    assert_eq!(*harness.state(), 1.001);

    press(&mut harness, Key::Enter);
    assert_eq!(*harness.state(), 1.001);
}

#[test]
fn float_focus_loss_keeps_full_precision() {
    let mut harness = Harness::new_ui_state(
        |ui, (first, second): &mut (f64, f64)| {
            ui.add(WheelBox::new(first).decimals(2));
            ui.add(WheelBox::new(second).decimals(2));
        },
        (1.23456, 2.0),
    );
    harness.run();

    focus(&mut harness, "2.00");
    focus(&mut harness, "1.23");
    focus(&mut harness, "2.00");
    assert_eq!(*harness.state(), (1.23456, 2.0));
}

#[test]
fn float_range_is_respected() {
    let mut harness = float_harness(0.5, |w| w.range([0.0, 1.0]));
    focus(&mut harness, "0.50");

    wheel(&mut harness, 1.0);
    assert_eq!(*harness.state(), 1.0);
}
