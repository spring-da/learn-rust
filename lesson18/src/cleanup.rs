use std::cell::RefCell;
use std::rc::Rc;

// 仅记录单线程演示的顺序，不是真实文件或连接。
#[derive(Clone, Default)]
pub struct Trace {
    events: Rc<RefCell<Vec<String>>>,
}

impl Trace {
    pub fn record(&self, event: impl Into<String>) {
        self.events.borrow_mut().push(event.into());
    }

    pub fn events(&self) -> Vec<String> {
        self.events.borrow().clone()
    }
}

pub struct TraceResource {
    name: &'static str,
    trace: Trace,
}

impl TraceResource {
    pub fn open(name: &'static str, trace: &Trace) -> Self {
        trace.record(format!("open:{name}"));
        Self { name, trace: trace.clone() }
    }
}

impl Drop for TraceResource {
    fn drop(&mut self) {
        self.trace.record(format!("close:{}", self.name));
    }
}

pub fn cleanup_scope(trace: &Trace) {
    let _outer = TraceResource::open("outer", trace);
    {
        let _inner = TraceResource::open("inner", trace);
        trace.record("body:inner");
    }
    trace.record("body:outer");
}

pub fn early_return(trace: &Trace, reject: bool) -> Result<(), &'static str> {
    let _resource = TraceResource::open("validation", trace);
    validate(trace, reject)?;
    trace.record("accept");
    Ok(())
}

fn validate(trace: &Trace, reject: bool) -> Result<(), &'static str> {
    if reject {
        trace.record("reject");
        return Err("document rejected");
    }
    Ok(())
}
