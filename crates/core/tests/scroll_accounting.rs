#[path = "../benches/harness/composite_tally.rs"]
mod composite_tally;

use composite_tally::CompositeTally;

#[test]
fn composites_accumulate_across_cache_incarnations() {
    let mut tally = CompositeTally::default();

    tally.observe(7, 1, 1);
    tally.observe(7, 1, 3);
    tally.observe(7, 2, 2);

    assert_eq!(tally.total(), 5);
    assert_eq!(tally.pages(), 1);
}

#[test]
fn repeated_observations_only_add_the_delta() {
    let mut tally = CompositeTally::default();

    tally.observe(7, 1, 1);
    tally.observe(7, 1, 3);
    tally.observe(7, 1, 3);

    assert_eq!(tally.total(), 3);
}

#[test]
#[should_panic(expected = "composite counter decreased")]
fn a_counter_decrease_within_one_incarnation_fails_loud() {
    let mut tally = CompositeTally::default();

    tally.observe(7, 1, 3);
    tally.observe(7, 1, 2);
}

#[test]
#[should_panic(expected = "cache incarnation decreased")]
fn an_incarnation_decrease_fails_loud() {
    let mut tally = CompositeTally::default();

    tally.observe(7, 2, 1);
    tally.observe(7, 1, 2);
}
