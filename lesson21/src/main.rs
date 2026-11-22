use lesson21::{MemorySender, send_text};

fn main() -> Result<(), lesson21::Error> {
    let text = std::env::args().nth(1).unwrap_or_else(|| "  模块边界清楚，调用方保持简单  ".into());
    let mut sender = MemorySender::new("local")?;
    let receipt = send_text(&mut sender, &text)?;
    println!("channel={} sequence={} text={:?}", receipt.channel, receipt.sequence, receipt.text);
    Ok(())
}
