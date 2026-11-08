use crate::{
    cancel::CancelToken,
    errors::QueryError,
    repository::{User, UserRepository},
};
use std::{
    sync::{Mutex, mpsc},
    thread,
};

struct Job {
    index: usize,
    id: i64,
}

pub struct LookupResult {
    pub index: usize,
    pub id: i64,
    pub outcome: Result<Option<User>, QueryError>,
}

pub fn find_with_pool(
    token: &CancelToken,
    repo: &dyn UserRepository,
    ids: &[i64],
    workers: usize,
) -> Result<Vec<LookupResult>, &'static str> {
    if workers == 0 {
        return Err("workers must be greater than zero");
    }
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let workers = workers.min(ids.len());
    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(workers);
    let jobs_rx = Mutex::new(jobs_rx);
    let (results_tx, results_rx) = mpsc::sync_channel(0);

    let all = thread::scope(|scope| {
        scope.spawn(move || {
            for (index, &id) in ids.iter().enumerate() {
                jobs_tx
                    .send(Job { index, id })
                    .expect("workers receive every submitted job");
            }
        });

        for _ in 0..workers {
            let jobs_rx = &jobs_rx;
            let results_tx = results_tx.clone();
            scope.spawn(move || {
                loop {
                    let next = {
                        let receiver = jobs_rx.lock().expect("job receiver lock is not poisoned");
                        receiver.recv()
                    };
                    let Ok(job) = next else {
                        break;
                    };
                    let result = LookupResult {
                        index: job.index,
                        id: job.id,
                        outcome: repo.find_by_id(token, job.id),
                    };
                    // 持续交回结果，取消后的查询入口会快速返回错误。
                    results_tx
                        .send(result)
                        .expect("collector receives every result");
                }
            });
        }

        drop(results_tx);
        let mut all: Vec<LookupResult> = results_rx.into_iter().collect();
        all.sort_by_key(|result| result.index);
        all
    });
    Ok(all)
}
