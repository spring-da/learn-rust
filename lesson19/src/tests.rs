use super::{
    ChannelLabel, ChannelNamer, EmailSender, Message, Receipt, SendError, Sender, SmsSender,
    describe, dispatch, make_sender, optional_sender, send_batch, send_batch_flexible,
};

fn message() -> Message { Message::new("SpringDa", "学习接口与 trait") }

#[test]
fn concrete_and_trait_object_borrows_share_one_instance() {
    let mut email = EmailSender::default();
    assert_eq!(email.send(&message()).unwrap().sequence, 1);
    let borrowed: &mut dyn Sender = &mut email;
    assert_eq!(borrowed.send(&message()).unwrap().sequence, 2);
    assert_eq!(email.sent(), 2);
}

#[test]
fn batches_work_for_concrete_type_and_explicit_unsized_trait_object() {
    let messages = [message(), message()];
    let mut email = EmailSender::default();
    let receipts = send_batch(&mut email, &messages).unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].channel, "email");
    assert_eq!(receipts[1].sequence, 2);

    let mut erased: Box<dyn Sender> = Box::new(SmsSender::default());
    let receipts = send_batch_flexible(&mut *erased, &messages).unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[1].channel, "sms");
    assert_eq!(receipts[1].sequence, 2);
}

struct RejectingSender;

impl ChannelNamer for RejectingSender {
    fn channel(&self) -> &'static str { "reject" }
}

impl Sender for RejectingSender {
    fn send(&mut self, _: &Message) -> Result<Receipt, SendError> {
        Err(SendError::InvalidMessage("injected rejection"))
    }

    fn fresh(&self) -> Self { Self }
}

#[test]
fn heterogeneous_dispatch_continues_after_failure_and_preserves_each_instance() {
    let mut senders: Vec<Box<dyn Sender>> = vec![
        Box::new(EmailSender::default()), Box::new(RejectingSender), Box::new(SmsSender::default()),
    ];
    for sequence in [1, 2] {
        let results = dispatch(&mut senders, &message());
        assert_eq!(results.len(), 3);
        let email = results[0].as_ref().unwrap();
        let sms = results[2].as_ref().unwrap();
        assert_eq!((email.channel, email.sequence), ("email", sequence));
        assert_eq!((sms.channel, sms.sequence), ("sms", sequence));
        assert_eq!(results[1], Err(SendError::InvalidMessage("injected rejection")));
    }
}

#[test]
fn batch_stops_on_error_without_rolling_back_prior_state() {
    let mut email = EmailSender::default();
    let result = send_batch(&mut email, &[message(), Message::new("dev", ""), message()]);
    assert_eq!(result, Err(SendError::InvalidMessage("text is empty")));
    assert_eq!(email.sent(), 1);
}

#[test]
fn factory_selects_implementation_and_rejects_unknown_channel() {
    for (input, expected) in [("email", "email"), (" SMS ", "sms")] {
        let mut sender = make_sender(input).unwrap();
        let receipt = sender.send(&message()).unwrap();
        assert_eq!((receipt.channel, receipt.sequence), (expected, 1));
    }
    assert!(matches!(make_sender("unknown"), Err(SendError::UnknownChannel(value)) if value == "unknown"));
}

#[test]
fn absence_is_explicit_and_enabled_sender_is_usable() {
    assert!(optional_sender(false).is_none());
    let mut sender = optional_sender(true).unwrap();
    assert_eq!(sender.send(&message()).unwrap().channel, "email");
}

#[test]
fn sized_only_fresh_method_resets_state_without_preventing_dynamic_send() {
    let mut email = EmailSender::default();
    email.send(&message()).unwrap();
    let fresh = email.fresh();
    assert_eq!(email.sent(), 1);
    assert_eq!(fresh.sent(), 0);
    let mut sender: Box<dyn Sender> = Box::new(fresh);
    assert_eq!(sender.send(&message()).unwrap().sequence, 1);
}

#[test]
fn narrow_trait_supports_labels_and_full_senders() {
    assert_eq!(describe(&ChannelLabel("preview")), "channel=preview");
    assert_eq!(describe(&EmailSender::default()), "channel=email");
}

#[test]
fn invalid_messages_do_not_advance_counters_and_unicode_is_preserved() {
    let mut senders: Vec<Box<dyn Sender>> = vec![
        Box::new(EmailSender::default()), Box::new(SmsSender::default()),
    ];
    for sender in &mut senders {
        assert_eq!(sender.send(&Message::new("　", "hello")), Err(SendError::InvalidMessage("recipient is empty")));
        assert_eq!(sender.send(&Message::new("dev", "\t")), Err(SendError::InvalidMessage("text is empty")));
        let receipt = sender.send(&message()).unwrap();
        assert_eq!(receipt.sequence, 1);
        assert_eq!(receipt.text, message().text);
    }
}
