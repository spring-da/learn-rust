use crate::{Error, Message, Receipt};

pub trait Sender {
    fn send(&mut self, message: &Message) -> Result<Receipt, Error>;
}

pub fn send_text(sender: &mut impl Sender, text: &str) -> Result<Receipt, Error> {
    let message = Message::new(text)?;
    sender.send(&message)
}
