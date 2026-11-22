use lesson23::{Pool, Snapshot, load_records, save_records};
use std::time::Duration;

fn display(records: &[Snapshot]) {
    for record in records { println!("id={} status={} attempts={} error={:?}", record.id, record.status.as_str(), record.attempts, record.error); }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = match args.as_slice() {
        [] => "data/results.tsv",
        [flag, path] if flag == "--out" => path,
        [flag, path] if flag == "--read" => { display(&load_records(path)?); return Ok(()); }
        _ => return Err("usage: lesson23 [--out path | --read path]".into()),
    };
    let mut pool = Pool::new(2, 8)?;
    let tickets = [
        pool.submit(1, |_, _| Ok(()))?,
        pool.submit(3, |_, attempt| if attempt <= 2 { Err("temporary failure".into()) } else { Ok(()) })?,
        pool.submit(2, |_, _| Err("permanent failure".into()))?,
        pool.submit(1, |cancel, _| { while !cancel.is_cancelled() { std::thread::sleep(Duration::from_millis(2)); } Err("cancelled".into()) })?,
    ];
    tickets[3].cancel();
    let mut records = Vec::new();
    for ticket in tickets { records.push(ticket.wait(Duration::from_secs(3))?); }
    pool.close()?;
    display(&records);
    save_records(out, &records)?;
    let restored = load_records(out)?;
    println!("saved={out:?} restored_terminal_records={}", restored.len());
    Ok(())
}
