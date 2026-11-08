mod cancel;
mod errors;
mod pool;
mod repository;

use cancel::CancelToken;
use pool::find_with_pool;
use repository::{MemoryUserRepository, UserRepository};
use std::{
    thread,
    time::{Duration, Instant},
};

fn run_case(label: &str, token: &CancelToken, repo: &dyn UserRepository, ids: &[i64]) {
    let start = Instant::now();
    let results = match find_with_pool(token, repo, ids, 2) {
        Ok(results) => results,
        Err(err) => {
            println!("pool error: {err}");
            return;
        }
    };
    let stop = match token.check() {
        Ok(()) => "none",
        Err(error) => error.as_str(),
    };
    println!(
        "case={label} count={} elapsed={}ms stop={stop}",
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

fn main() {
    let repo = MemoryUserRepository::new();
    let ids = [1, 2, 1, 404, 500];
    run_case("normal", &CancelToken::new(), &repo, &ids);

    let timeout = CancelToken::with_timeout(Duration::from_millis(200));
    run_case("timeout", &timeout, &repo, &ids);

    let manual = CancelToken::new();
    thread::scope(|scope| {
        scope.spawn(|| {
            thread::sleep(Duration::from_millis(100));
            manual.cancel();
        });
        run_case("manual-cancel", &manual, &repo, &ids);
    });

    let already_cancelled = CancelToken::new();
    already_cancelled.cancel();
    run_case("already-cancelled", &already_cancelled, &repo, &ids);
}
