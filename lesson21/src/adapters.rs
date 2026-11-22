use crate::{Error, Message, Receipt, Sender, normalize_channel};

pub struct MemorySender { channel: String, sent: usize }

impl MemorySender {
    pub fn new(channel: &str) -> Result<Self, Error> {
        Ok(Self { channel: normalize_channel(channel)?, sent: 0 })
    }
}

impl Sender for MemorySender {
    fn send(&mut self, message: &Message) -> Result<Receipt, Error> {
        self.sent += 1;
        Ok(Receipt { channel: self.channel.clone(), sequence: self.sent, text: message.text().into() })
    }
}
