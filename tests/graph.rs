use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_widgets::{Graph, GraphBounds, GraphData, GraphSeries, GraphState};

struct App {
    state: GraphState,
    data: GraphData,
    enabled: bool,
    rect: Rect,
    changed: bool,
}

fn app(state: GraphState) -> App {
    let values = (0..10_000).map(|i| (i as f64 * 0.01).sin()).collect();
    App {
        state,
        data: GraphData::from_uniform(0.0, 0.001, values).unwrap(),
        enabled: true,
        rect: Rect::NOTHING,
        changed: false,
    }
}

fn harness(state: GraphState) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(vec2(500.0, 400.0))
        .build_ui_state(
            |ui, app: &mut App| {
                ui.add_enabled_ui(app.enabled, |ui| {
                    let series = [GraphSeries::new("sine", &app.data)];
                    let response = Graph::new(&mut app.state)
                        .series(&series)
                        .desired_size([400.0, 300.0])
                        .show(ui);
                    assert!(response.error.is_none());
                    app.rect = response.response.rect;
                    app.changed |= response.response.changed();
                });
            },
            app(state),
        );
    harness.run();
    harness
}

fn bounds(x: [f64; 2], y: [f64; 2]) -> GraphBounds {
    GraphBounds::new(x, y).unwrap()
}

fn fixed() -> GraphState {
    GraphState::new(bounds([0.0, 10.0], [-1.0, 1.0]))
}

fn move_to(harness: &mut Harness<'_, App>, pos: Pos2) {
    harness.event(Event::PointerMoved(pos));
    harness.run();
}

fn button(harness: &mut Harness<'_, App>, button: PointerButton, pos: Pos2, pressed: bool) {
    harness.event(Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

/// Drag with `b` from `from` by `delta`, in `steps` moves.
fn drag(
    harness: &mut Harness<'_, App>,
    b: PointerButton,
    from: Pos2,
    delta: egui::Vec2,
    steps: usize,
) {
    move_to(harness, from);
    button(harness, b, from, true);
    for step in 1..=steps {
        move_to(harness, from + delta * step as f32 / steps as f32);
    }
    button(harness, b, from + delta, false);
}

/// Data coordinates of a screen position.
fn data_at(b: GraphBounds, rect: Rect, pos: Pos2) -> [f64; 2] {
    let fx = f64::from((pos.x - rect.min.x) / rect.width());
    let fy = f64::from((pos.y - rect.min.y) / rect.height());
    [b.x()[0] + fx * b.x_span(), b.y()[1] - fy * b.y_span()]
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn default_state_fits_the_data() {
    let harness = harness(GraphState::default());
    let b = harness.state().state.bounds();
    assert!(b.x()[0] < 0.0 && b.x()[1] > 9.999);
    assert!(b.y()[0] < -1.0 && b.y()[1] > 1.0);
    assert!(b.x_span() < 9.999 * 1.11);
}

#[test]
fn explicit_bounds_and_fit() {
    let mut harness = harness(fixed());
    assert_eq!(
        harness.state().state.bounds(),
        bounds([0.0, 10.0], [-1.0, 1.0])
    );

    let app = harness.state_mut();
    let series = [GraphSeries::new("sine", &app.data)];
    assert!(app.state.fit_to_data(&series));
    let fitted = app.state.bounds();
    assert!(fitted.x()[0] < 0.0);
    harness.run();
    assert_eq!(harness.state().state.bounds(), fitted);

    let empty = GraphData::from_xy(Vec::new()).unwrap();
    let app = harness.state_mut();
    assert!(!app.state.fit_to_data(&[GraphSeries::new("empty", &empty)]));
    assert_eq!(app.state.bounds(), fitted);
}

#[test]
fn left_drag_pans_with_the_pointer() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    drag(
        &mut harness,
        PointerButton::Primary,
        rect.center(),
        vec2(40.0, 30.0),
        4,
    );
    let b = harness.state().state.bounds();
    let dx = -40.0 / f64::from(rect.width()) * 10.0;
    let dy = 30.0 / f64::from(rect.height()) * 2.0;
    assert!(close(b.x()[0], dx) && close(b.x()[1], 10.0 + dx), "{b:?}");
    assert!(
        close(b.y()[0], -1.0 + dy) && close(b.y()[1], 1.0 + dy),
        "{b:?}"
    );
    assert!(harness.state().changed);
    assert!(!harness.state().state.is_dragging());
}

#[test]
fn right_drag_scales_each_axis() {
    let cases = [
        (vec2(30.0, 0.0), [true, false], [false, false]),
        (vec2(-30.0, 0.0), [true, false], [true, false]),
        (vec2(0.0, 30.0), [false, true], [false, true]),
        (vec2(0.0, -30.0), [false, true], [false, false]),
        (vec2(30.0, 30.0), [true, true], [false, true]),
    ];
    for (delta, changes, grows) in cases {
        let mut harness = harness(fixed());
        let rect = harness.state().rect;
        drag(
            &mut harness,
            PointerButton::Secondary,
            rect.center(),
            delta,
            3,
        );
        let b = harness.state().state.bounds();
        let spans = [b.x_span(), b.y_span()];
        for axis in 0..2 {
            let before = [10.0, 2.0][axis];
            if changes[axis] {
                assert_eq!(spans[axis] > before, grows[axis], "{delta:?} {b:?}");
            } else {
                assert!(close(spans[axis], before), "{delta:?} {b:?}");
            }
        }
        // Dragging right zooms into X by 1.02 per point, as in PyQtGraph.
        if delta == vec2(30.0, 0.0) {
            assert!(close(b.x_span(), 10.0 * 1.02_f64.powf(-30.0)));
        }
    }
}

#[test]
fn right_drag_keeps_the_press_position() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    let origin = rect.min + vec2(100.0, 70.0);
    let before = data_at(harness.state().state.bounds(), rect, origin);
    drag(
        &mut harness,
        PointerButton::Secondary,
        origin,
        vec2(25.0, -40.0),
        5,
    );
    let after = data_at(harness.state().state.bounds(), rect, origin);
    assert!(close(before[0], after[0]) && close(before[1], after[1]));
}

#[test]
fn result_does_not_depend_on_frames() {
    for b in [PointerButton::Primary, PointerButton::Secondary] {
        let mut results = Vec::new();
        for steps in [1, 2, 9] {
            let mut harness = harness(fixed());
            let rect = harness.state().rect;
            drag(&mut harness, b, rect.center(), vec2(37.0, -23.0), steps);
            results.push(harness.state().state.bounds());
        }
        assert!(results.windows(2).all(|w| w[0] == w[1]), "{results:?}");
    }
}

#[test]
fn drag_continues_outside_of_the_graph() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    let from = rect.center();
    move_to(&mut harness, from);
    button(&mut harness, PointerButton::Primary, from, true);
    move_to(&mut harness, from + vec2(20.0, 0.0));
    let outside = pos2(rect.max.x + 50.0, from.y);
    move_to(&mut harness, outside);
    let b = harness.state().state.bounds();
    let dx = -f64::from(outside.x - from.x) / f64::from(rect.width()) * 10.0;
    assert!(close(b.x()[0], dx), "{b:?}");
    button(&mut harness, PointerButton::Primary, outside, false);
    assert!(!harness.state().state.is_dragging());

    // Moving after the release changes nothing.
    move_to(&mut harness, from);
    assert_eq!(harness.state().state.bounds(), b);
}

#[test]
fn set_bounds_cancels_the_drag() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    let from = rect.center();
    move_to(&mut harness, from);
    button(&mut harness, PointerButton::Primary, from, true);
    move_to(&mut harness, from + vec2(20.0, 0.0));
    assert!(harness.state().state.is_dragging());

    let set = bounds([5.0, 6.0], [0.0, 1.0]);
    harness.state_mut().state.set_bounds(set);
    move_to(&mut harness, from + vec2(60.0, 10.0));
    assert_eq!(harness.state().state.bounds(), set);
    button(
        &mut harness,
        PointerButton::Primary,
        from + vec2(60.0, 10.0),
        false,
    );
    assert_eq!(harness.state().state.bounds(), set);
}

#[test]
fn disabled_graph_ignores_the_pointer() {
    let mut harness = harness(fixed());
    harness.state_mut().enabled = false;
    harness.run();
    let rect = harness.state().rect;
    drag(
        &mut harness,
        PointerButton::Primary,
        rect.center(),
        vec2(40.0, 0.0),
        3,
    );
    drag(
        &mut harness,
        PointerButton::Secondary,
        rect.center(),
        vec2(40.0, 0.0),
        3,
    );
    assert_eq!(
        harness.state().state.bounds(),
        bounds([0.0, 10.0], [-1.0, 1.0])
    );
}

#[test]
fn disabling_cancels_the_drag() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    let from = rect.center();
    move_to(&mut harness, from);
    button(&mut harness, PointerButton::Secondary, from, true);
    move_to(&mut harness, from + vec2(20.0, 0.0));
    let b = harness.state().state.bounds();
    harness.state_mut().enabled = false;
    move_to(&mut harness, from + vec2(60.0, 0.0));
    assert!(!harness.state().state.is_dragging());
    assert_eq!(harness.state().state.bounds(), b);
}

#[test]
fn extreme_zoom_stays_valid() {
    let mut harness = harness(GraphState::new(bounds([1.7e9, 1.7e9 + 1.0], [-1.0, 1.0])));
    let rect = harness.state().rect;
    // Far enough for 1.02^-x to underflow, then to overflow.
    for delta in [vec2(5000.0, -5000.0), vec2(-5000.0, 5000.0)] {
        drag(
            &mut harness,
            PointerButton::Secondary,
            rect.center(),
            delta,
            2,
        );
        let b = harness.state().state.bounds();
        assert!(GraphBounds::new(b.x(), b.y()).is_ok(), "{b:?}");
        assert!(b.x_span() > 0.0 && b.x_span().is_finite());
    }
}

#[test]
fn unchanged_frames_do_no_work() {
    let mut harness = harness(fixed());
    harness.run();
    let stats = harness.state().state.cache().prepared().stats();
    assert_eq!(stats.samples_read, 0);
    assert_eq!(
        [
            stats.line_selections,
            stats.marker_selections,
            stats.mesh_rebuilds
        ],
        [0, 0, 0]
    );
    assert!(stats.vertices > 0);
}

#[test]
fn focus_loss_cancels_the_drag() {
    let mut harness = harness(fixed());
    let rect = harness.state().rect;
    let from = rect.center();
    move_to(&mut harness, from);
    button(&mut harness, PointerButton::Primary, from, true);
    move_to(&mut harness, from + vec2(20.0, 0.0));
    assert!(harness.state().state.is_dragging());
    let b = harness.state().state.bounds();
    harness.event(Event::WindowFocused(false));
    harness.run();
    assert!(!harness.state().state.is_dragging());
    move_to(&mut harness, from + vec2(60.0, 0.0));
    assert_eq!(harness.state().state.bounds(), b);
}
