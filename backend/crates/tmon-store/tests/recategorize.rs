mod common;

use common::*;
use std::sync::Mutex;
use tmon_core::{Categorizer, Direction, FlowRecord};
use tmon_store::FlowStore;
use tmon_store::duck::recategorize::recategorize;

/// Port 443 → category 7 / rule 42, everything else unchanged.
struct Https;
impl Categorizer for Https {
    fn categorize(&self, f: &FlowRecord) -> (u16, Option<u32>) {
        if f.remote_port == 443 {
            (7, Some(42))
        } else {
            (f.category_id, f.rule_id)
        }
    }
}

fn seed() -> (tempfile::TempDir, std::sync::Arc<tmon_store::DuckStore>) {
    let (d, s) = open();
    let mut flows = Vec::new();
    for h in 0..3 {
        flows.push(rec(
            T0 + h * HOUR + 1000,
            "192.168.88.10",
            "1.1.1.1",
            443,
            1000,
            1,
            Direction::Down,
        ));
        flows.push(rec(
            T0 + h * HOUR + 2000,
            "192.168.88.10",
            "1.1.1.1",
            53,
            10,
            1,
            Direction::Down,
        ));
    }
    s.append_batch(&flows).unwrap();
    s.rotate(T0 + 2 * HOUR).unwrap(); // hours 0,1 → parquet; hour 2 stays hot
    (d, s)
}

#[test]
fn reapply_rewrites_parquet_hot_and_rollups() {
    let (_d, s) = seed();
    let seen = Mutex::new(Vec::new());
    let n = recategorize(&s, T0, T0 + 3 * HOUR, &Https, &|p| {
        seen.lock().unwrap().push(p)
    })
    .unwrap();
    assert_eq!(n, 3);
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE category_id = 7 AND rule_id = 42"
        ),
        3
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM flows WHERE remote_port = 53 AND category_id = 1"
        ),
        3
    );
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows_hot WHERE category_id = 7"),
        1
    );
    for g in ["1m", "1h"] {
        assert_eq!(
            count(
                &s,
                &format!(
                    "SELECT sum(bytes)::BIGINT FROM rollup_category_{g} WHERE category_id = 7"
                )
            ),
            3000
        );
        assert_eq!(
            count(
                &s,
                &format!("SELECT sum(bytes)::BIGINT FROM rollup_category_{g}")
            ),
            3030,
            "totals preserved ({g})"
        );
        assert_eq!(
            count(
                &s,
                &format!("SELECT sum(bytes)::BIGINT FROM rollup_host_{g} WHERE category_id = 1")
            ),
            30
        );
        assert_eq!(
            count(
                &s,
                &format!("SELECT sum(bytes)::BIGINT FROM rollup_rule_{g} WHERE rule_id = 42")
            ),
            3000
        );
    }
    let p = seen.lock().unwrap().clone();
    assert_eq!(p.first(), Some(&0.0));
    assert_eq!(p.last(), Some(&1.0));
    assert!(p.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn partial_range_only_touches_those_hours() {
    let (_d, s) = seed();
    recategorize(&s, T0 + HOUR, T0 + 2 * HOUR, &Https, &|_| {}).unwrap();
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows WHERE category_id = 7"),
        1
    );
    assert_eq!(
        count(
            &s,
            &format!(
                "SELECT count(*) FROM rollup_category_1h WHERE category_id = 7 AND bucket = {}",
                T0 + HOUR
            )
        ),
        1
    );
}

#[test]
fn failure_leaves_original_file_intact() {
    let (d, s) = seed();
    let files: Vec<_> = walk(&d.path().join("raw"));
    let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    // A temp path that is a directory makes the COPY fail.
    std::fs::create_dir(files[0].with_extension("parquet.tmp")).unwrap();
    assert!(recategorize(&s, T0, T0 + HOUR, &Https, &|_| {}).is_err());
    assert_eq!(std::fs::read(&files[0]).unwrap(), before[0]);
    assert_eq!(
        count(&s, "SELECT count(*) FROM flows WHERE category_id = 7"),
        0
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p))
        } else {
            out.push(p)
        }
    }
    out.sort();
    out
}
