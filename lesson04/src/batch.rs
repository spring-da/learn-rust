use crate::repository::{User, UserRepository};
use std::{sync::mpsc, thread};

pub struct LookupResult {
    pub id: i64,
    pub outcome: Result<Option<User>, &'static str>,
}

fn lookup(repo: &dyn UserRepository, id: i64) -> LookupResult {
    LookupResult {
        id,
        outcome: repo.find_by_id(id),
    }
}

pub fn find_sequentially(repo: &dyn UserRepository, ids: &[i64]) -> Vec<LookupResult> {
    let mut all = Vec::with_capacity(ids.len());
    for &id in ids {
        all.push(lookup(repo, id));
    }
    all
}

pub fn find_concurrently(repo: &dyn UserRepository, ids: &[i64]) -> Vec<LookupResult> {
    // 容量 0：发送需等待接收方，与本课 Go 的无缓冲 channel 对照。
    let (tx, rx) = mpsc::sync_channel(0);

    thread::scope(|scope| {
        for &id in ids {
            let tx = tx.clone();
            scope.spawn(move || {
                let result = lookup(repo, id);
                tx.send(result)
                    .expect("receiver stays alive until all results arrive");
            });
        }

        // 原始发送端不负责发送；释放它后，所有任务结束才会断开整个发送侧。
        drop(tx);

        // 当前线程持续接收；全部发送端被释放时，迭代结束。
        rx.into_iter().collect()
    })
}
