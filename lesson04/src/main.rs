mod batch;
mod repository;

use batch::{LookupResult, find_concurrently, find_sequentially};
use repository::MemoryUserRepository;
use std::time::{Duration, Instant};

fn print_results(label: &str, results: &[LookupResult], elapsed: Duration) {
    println!(
        "{label}: count={} elapsed={}ms",
        results.len(),
        elapsed.as_millis()
    );
    for result in results {
        match &result.outcome {
            Ok(Some(user)) => println!("  id={} found={}", result.id, user.name),
            Ok(None) => println!("  id={} not-found", result.id),
            Err(err) => println!("  id={} error={err}", result.id),
        }
    }
}

fn main() {
    let repo = MemoryUserRepository::new();
    let ids = [1, 2, 404, 500];

    let start = Instant::now();
    let sequential = find_sequentially(&repo, &ids);
    print_results("sequential", &sequential, start.elapsed());

    let start = Instant::now();
    let concurrent = find_concurrently(&repo, &ids);
    print_results("concurrent", &concurrent, start.elapsed());
}
