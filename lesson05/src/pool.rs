use crate::repository::{User, UserRepository};
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
    pub outcome: Result<Option<User>, &'static str>,
}

pub fn find_with_pool(
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
    let workers = workers.min(ids.len());   // 两个 usize 比较取最小

    let (jobs_tx, jobs_rx) = mpsc::sync_channel::<Job>(workers);
    let jobs_rx = Mutex::new(jobs_rx);
    let (results_tx, results_rx) = mpsc::sync_channel(0);

    let all = thread::scope(|scope| {
        scope.spawn(move || {   // scope.spawn 可以借用作用域外的变量   普通 thread::spawn：它要求闭包是 'static 的（不能借用外部引用  move 是闭包捕获方式的修饰符。它决定闭包如何"拿走"外部变量  不加 move ：按引用借用 加 move：按值移动（或复制）
            for (index, &id) in ids.iter().enumerate() {
                jobs_tx
                    .send(Job { index, id })
                    .expect("workers receive every submitted job");
            }
            // jobs_tx 离开作用域，worker 取完已排队任务后 recv 返回 Err。
        });

        for _ in 0..workers {
            let jobs_rx = &jobs_rx;
            let results_tx = results_tx.clone();
            scope.spawn(move || {
                loop {
                    let next = {
                        let receiver = jobs_rx.lock().expect("job receiver lock is not poisoned");
                        receiver.recv()
                    }; // guard 离开作用域，在执行查询之前自动解锁。

                    let Ok(job) = next else {
                        break;
                    };
                    let result = LookupResult {
                        index: job.index,
                        id: job.id,
                        outcome: repo.find_by_id(job.id),
                    };
                    results_tx
                        .send(result)
                        .expect("collector receives every result");
                }
            });
        }

        drop(results_tx);   // 确保所有 clone 的results_tx drop 避免死锁
        let mut all: Vec<LookupResult> = results_rx.into_iter().collect();
        all.sort_by_key(|result| result.index);
        all
    });
    Ok(all)
}
