use crate::{Error, Snapshot, Status};
use std::collections::HashSet;
use std::path::Path;

const HEADER: &str = "lesson23-results-v1";

fn validate(records: &[Snapshot]) -> Result<(), Error> {
    let mut seen = HashSet::new();
    for record in records {
        if record.id == 0 || !seen.insert(record.id) || !record.status.terminal() || record.attempts > 8 { return Err(Error::Record); }
        match record.status {
            Status::Succeeded if record.attempts == 0 || !record.error.is_empty() => return Err(Error::Record),
            Status::Failed if record.attempts == 0 || record.error.is_empty() => return Err(Error::Record),
            Status::Cancelled if record.error.is_empty() => return Err(Error::Record),
            _ => {}
        }
    }
    Ok(())
}

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut result = String::with_capacity(data.len() * 2);
    for byte in data { result.push(HEX[(byte >> 4) as usize] as char); result.push(HEX[(byte & 15) as usize] as char); }
    result
}

fn hex_decode(text: &str) -> Result<String, Error> {
    if text.len() % 2 != 0 { return Err(Error::Record); }
    let mut bytes = Vec::new();
    for pair in text.as_bytes().chunks_exact(2) {
        let digit = |byte: u8| (byte as char).to_digit(16).ok_or(Error::Record);
        bytes.push((digit(pair[0])? * 16 + digit(pair[1])?) as u8);
    }
    String::from_utf8(bytes).map_err(|_| Error::Record)
}

pub fn save_records(path: impl AsRef<Path>, records: &[Snapshot]) -> Result<(), Error> {
    validate(records)?;
    let mut data = format!("{HEADER}\n");
    for record in records {
        data.push_str(&format!("{}\t{}\t{}\t{}\n", record.id, record.status.as_str(), record.attempts, hex_encode(record.error.as_bytes())));
    }
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { std::fs::create_dir_all(parent)?; }
    std::fs::write(path, data)?;
    Ok(())
}

pub fn load_records(path: impl AsRef<Path>) -> Result<Vec<Snapshot>, Error> {
    let data = std::fs::read_to_string(path)?;
    let mut lines: Vec<_> = data.split('\n').collect();
    if lines.len() < 2 || lines[0] != HEADER || lines.pop() != Some("") { return Err(Error::Record); }
    let mut records = Vec::new();
    for line in &lines[1..] {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 { return Err(Error::Record); }
        let id = fields[0].parse::<u64>().map_err(|_| Error::Record)?;
        let attempts = fields[2].parse::<usize>().map_err(|_| Error::Record)?;
        records.push(Snapshot { id, status: Status::parse(fields[1])?, attempts, error: hex_decode(fields[3])? });
    }
    validate(&records)?;
    Ok(records)
}
