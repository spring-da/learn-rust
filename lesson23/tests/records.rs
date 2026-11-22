use lesson23::{Error, Snapshot, Status, load_records, save_records};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("codex-lesson23-{}-{}.tsv", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }

#[test]
fn terminal_records_round_trip_unicode_and_separators() {
    let fixture = Fixture::new();
    let records = vec![
        Snapshot { id: 1, status: Status::Succeeded, attempts: 1, error: "".into() },
        Snapshot { id: 2, status: Status::Failed, attempts: 2, error: "失败\t原因\n重试".into() },
        Snapshot { id: 3, status: Status::Cancelled, attempts: 0, error: "cancelled".into() },
    ];
    save_records(&fixture.0, &records).unwrap();
    assert_eq!(load_records(&fixture.0).unwrap(), records);
}

#[test]
fn invalid_states_duplicates_and_broken_files_are_rejected() {
    let fixture = Fixture::new();
    for (status, attempts, error) in [
        (Status::Running, 1, ""), (Status::Succeeded, 0, ""),
        (Status::Failed, 1, ""), (Status::Cancelled, 0, ""),
    ] {
        assert!(matches!(save_records(&fixture.0, &[Snapshot { id: 1, status, attempts, error: error.into() }]), Err(Error::Record)));
    }
    let valid = Snapshot { id: 1, status: Status::Succeeded, attempts: 1, error: String::new() };
    assert!(matches!(save_records(&fixture.0, &[valid.clone(), valid]), Err(Error::Record)));
    for data in [
        "bad\n", "lesson23-results-v1\n1\tsucceeded\t1\t",
        "lesson23-results-v1\n1\tfailed\t1\t0g\n",
        "lesson23-results-v1\n1\tfailed\t1\tff\n",
        "lesson23-results-v1\n0\tsucceeded\t1\t\n",
    ] {
        std::fs::write(&fixture.0, data).unwrap();
        assert!(matches!(load_records(&fixture.0), Err(Error::Record)));
    }
}
