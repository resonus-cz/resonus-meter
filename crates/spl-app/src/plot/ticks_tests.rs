use super::{TIME_STEPS_S, time_ticks};

#[test]
fn the_steps_increase() {
    assert!(TIME_STEPS_S.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn a_thirteen_second_window_gets_two_second_ticks() {
    // 1 s steps would give 14 ticks, 2 s steps 7
    assert_eq!(
        time_ticks(0.0, 13.0, 7),
        vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0]
    );
}

#[test]
fn ticks_start_at_the_first_multiple_inside_the_window() {
    assert_eq!(time_ticks(3.5, 9.0, 6), vec![4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    assert_eq!(time_ticks(61.0, 299.0, 5), vec![120.0, 180.0, 240.0]);
}

#[test]
fn bounds_on_a_multiple_are_included_within_rounding() {
    assert_eq!(
        time_ticks(10.0 - 1e-12, 20.0 + 1e-12, 3),
        vec![10.0, 15.0, 20.0]
    );
}

#[test]
fn an_hour_uses_clock_divisions() {
    let ticks = time_ticks(0.0, 3_600.0, 6);
    assert_eq!(ticks, vec![0.0, 900.0, 1_800.0, 2_700.0, 3_600.0]);
}

#[test]
fn very_long_windows_use_multiples_of_the_largest_step() {
    // 30 days, at most 5 ticks: 2 days give 16, so 8 days (4 x 2 days): 0, 8, 16, 24
    let day = 86_400.0;
    let ticks = time_ticks(0.0, 30.0 * day, 5);
    assert_eq!(ticks, vec![0.0, 8.0 * day, 16.0 * day, 24.0 * day]);
}

#[test]
fn a_window_shorter_than_one_step_may_have_one_tick_or_none() {
    assert_eq!(time_ticks(2.2, 2.8, 5), Vec::<f64>::new());
    assert_eq!(time_ticks(2.0, 2.0, 5), vec![2.0]);
}

#[test]
fn degenerate_inputs_give_no_ticks() {
    assert!(time_ticks(0.0, 10.0, 0).is_empty());
    assert!(time_ticks(10.0, 0.0, 5).is_empty());
    assert!(time_ticks(f64::NAN, 10.0, 5).is_empty());
    assert!(time_ticks(0.0, f64::INFINITY, 5).is_empty());
}
