use lesson19::{
    ChannelLabel, EmailSender, Message, SendError, Sender, SmsSender, describe, dispatch,
    make_sender, optional_sender, send_batch, send_batch_flexible,
};

fn main() -> Result<(), SendError> {
    let message = Message::new("SpringDa", "开始学习接口与 trait");
    let messages = [message.clone(), Message::new("JavaDev", "第二条消息")];
    println!("Lesson 19: traits, generics, and dispatch (in-memory simulation)");

    println!("\n[1] Explicit trait implementations and mutable receivers");
    let mut email = EmailSender::default();
    let first = email.send(&message)?;
    let borrowed: &mut dyn Sender = &mut email;
    let second = borrowed.send(&message)?;
    println!("same instance sequences={},{} total={}", first.sequence, second.sequence, email.sent());

    println!("\n[2] Concrete generic calls and a ?Sized generic call through dyn Sender");
    let concrete = send_batch(&mut EmailSender::default(), &messages)?;
    let mut erased: Box<dyn Sender> = Box::new(SmsSender::default());
    let dynamic = send_batch_flexible(&mut *erased, &messages)?;
    println!("T=EmailSender: {concrete:?}\nT=dyn Sender: {dynamic:?}");

    println!("\n[3] A heterogeneous collection owns different implementations");
    let mut senders: Vec<Box<dyn Sender>> = vec![
        Box::new(EmailSender::default()),
        Box::new(SmsSender::default()),
    ];
    for (index, result) in dispatch(&mut senders, &message).into_iter().enumerate() {
        println!("index={index} result={result:?}");
    }

    println!("\n[4] Select a concrete implementation at runtime");
    let kind = std::env::args().nth(1).unwrap_or_else(|| "email".into());
    let mut selected = make_sender(&kind)?;
    println!("kind={kind:?} receipt={:?}", selected.send(&message)?);

    println!("\n[5] Absence is represented explicitly with Option");
    let disabled = optional_sender(false);
    println!("disabled.is_none()={}", disabled.is_none());
    if let Some(mut enabled) = optional_sender(true) {
        println!("enabled receipt={:?}", enabled.send(&message)?);
    }

    println!("\n[6] Narrow traits and methods restricted to concrete Sized types");
    println!("label: {}; sender: {}", describe(&ChannelLabel("preview")), describe(&email));
    let fresh = email.fresh();
    println!("original count={} fresh count={}", email.sent(), fresh.sent());
    let mut boxed: Box<dyn Sender> = Box::new(fresh);
    println!("dyn send remains available: {:?}", boxed.send(&message)?);
    Ok(())
}
