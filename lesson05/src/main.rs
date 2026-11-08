mod pool;
mod repository;

use pool::find_with_pool;
use repository::MemoryUserRepository;
use std::time::Instant;

fn main() {
    let repo = MemoryUserRepository::new();
    let ids = [1, 2, 1, 404, 500];

    for workers in [1, 2] {
        let start = Instant::now();
        let results = match find_with_pool(&repo, &ids, workers) {
            Ok(results) => results,
            Err(err) => {
                println!("pool error: {err}");
                return;
            }
        };
        println!(
            "workers={workers} count={} elapsed={}ms",
            results.len(),
            start.elapsed().as_millis()
        );
        for result in results {
            match result.outcome {
                Ok(Some(user)) => println!(
                    "  index={} id={} found={}",
                    result.index, result.id, user.name
                ),
                Ok(None) => println!("  index={} id={} not-found", result.index, result.id),
                Err(err) => println!("  index={} id={} error={err}", result.index, result.id),
            }
        }
    }

    match find_with_pool(&repo, &[], 2) {
        Ok(results) => println!("empty: count={}", results.len()),
        Err(err) => println!("pool error: {err}"),
    }
    if let Err(err) = find_with_pool(&repo, &ids, 0) {
        println!("invalid workers=0: {err}");
    }
}
