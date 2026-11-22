use lesson21::{Error, MemorySender, Message, Receipt, Sender, send_text};

#[test]
fn facade_preserves_validation_and_adapter_state() {
    let mut sender = MemorySender::new(" local ").unwrap();
    assert!(matches!(send_text(&mut sender, " \t "), Err(Error::InvalidText)));
    for sequence in [1, 2] {
        let receipt = send_text(&mut sender, "  你好  ").unwrap();
        assert_eq!(receipt.channel, "local");
        assert_eq!(receipt.sequence, sequence);
        assert_eq!(receipt.text, "你好");
    }
    assert!(matches!(MemorySender::new(" "), Err(Error::InvalidChannel)));
}

#[test]
fn constructor_enforces_byte_boundary() {
    assert!(Message::new(&"你".repeat(40)).is_ok());
    assert!(matches!(Message::new(&"a".repeat(121)), Err(Error::InvalidText)));
}

struct RecordingSender { calls: usize }
impl Sender for RecordingSender {
    fn send(&mut self, message: &Message) -> Result<Receipt, Error> {
        self.calls += 1;
        Ok(Receipt { channel: "test".into(), sequence: self.calls, text: message.text().into() })
    }
}

#[test]
fn external_implementation_depends_only_on_public_contract() {
    let mut sender = RecordingSender { calls: 0 };
    assert!(send_text(&mut sender, " ").is_err());
    assert_eq!(sender.calls, 0);
    assert_eq!(send_text(&mut sender, "hello").unwrap().text, "hello");
}
