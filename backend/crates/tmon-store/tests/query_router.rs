use tmon_core::Direction;
use tmon_store::StoreError;
use tmon_store::duck::query::router::{Dim, Filter, Grain, Source, Windows, route};

const DAY: i64 = 86_400_000;
const NOW: i64 = 1_767_225_600_000;

fn f(span: i64) -> Filter {
    Filter::range(NOW - span, NOW)
}

#[test]
fn short_rollup_covered_range_uses_minute_grain() {
    let s = route(&f(DAY), &[Dim::Category], Windows::default(), NOW).unwrap();
    assert_eq!(
        s,
        Source::Rollup {
            family: "rollup_category",
            grain: Grain::Minute
        }
    );
    assert_eq!(s.label(), "rollup_1m");
}

#[test]
fn long_range_uses_hour_grain() {
    let s = route(&f(10 * DAY), &[Dim::Category], Windows::default(), NOW).unwrap();
    assert_eq!(s.label(), "rollup_1h");
    let old = Filter::range(NOW - 40 * DAY, NOW - 39 * DAY);
    assert_eq!(
        route(&old, &[], Windows::default(), NOW).unwrap().label(),
        "rollup_1h"
    );
}

#[test]
fn picks_the_family_covering_filter_and_group() {
    let mut flt = f(DAY);
    flt.hosts = vec!["10.0.0.1".into()];
    flt.directions = vec![Direction::Down];
    let s = route(&flt, &[Dim::Category], Windows::default(), NOW).unwrap();
    assert_eq!(
        s,
        Source::Rollup {
            family: "rollup_host",
            grain: Grain::Minute
        }
    );
    let geo = route(&f(DAY), &[Dim::Country], Windows::default(), NOW).unwrap();
    assert!(matches!(
        geo,
        Source::Rollup {
            family: "rollup_geo",
            ..
        }
    ));
    let port = route(&f(DAY), &[Dim::Port], Windows::default(), NOW).unwrap();
    assert!(matches!(
        port,
        Source::Rollup {
            family: "rollup_port",
            ..
        }
    ));
}

#[test]
fn uncovered_combination_within_raw_retention_uses_raw() {
    let mut flt = f(DAY);
    flt.hosts = vec!["10.0.0.1".into()];
    flt.countries = vec!["DE".into()];
    assert_eq!(
        route(&flt, &[], Windows::default(), NOW).unwrap(),
        Source::Raw
    );
}

#[test]
fn uncovered_combination_beyond_raw_retention_is_rejected() {
    let mut flt = f(10 * DAY);
    flt.hosts = vec!["10.0.0.1".into()];
    flt.countries = vec!["DE".into()];
    match route(&flt, &[Dim::Category], Windows::default(), NOW) {
        Err(StoreError::UnsupportedFilterRange { supported_dims }) => {
            assert!(supported_dims.contains(&"host".to_string()));
            assert!(supported_dims.contains(&"country".to_string()));
        }
        other => panic!("expected UnsupportedFilterRange, got {other:?}"),
    }
}

#[test]
fn rule_breakdown_uses_the_rule_rollup_beyond_raw_retention() {
    let s = route(&f(30 * DAY), &[Dim::Rule], Windows::default(), NOW).unwrap();
    assert_eq!(
        s,
        Source::Rollup {
            family: "rollup_rule",
            grain: Grain::Hour
        }
    );
    let cat = route(&f(DAY), &[Dim::Category], Windows::default(), NOW).unwrap();
    assert!(
        matches!(
            cat,
            Source::Rollup {
                family: "rollup_category",
                ..
            }
        ),
        "category stays on its own family"
    );
}
