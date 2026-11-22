use lesson18::cleanup::{Trace, TraceResource, cleanup_scope, early_return};
use lesson18::{Document, choose_preview, load_document};
use std::io;

fn main() -> io::Result<()> {
    println!("Lesson 18: document lifetime and resource cleanup");

    println!("\n[1] Return an owned Document by moving it to the caller");
    let mut doc = Document::new(String::from("TITLE: old\nbody"));
    println!("first={:?}", doc.first_line());

    println!("\n[2] Use a borrowed line, then retain an owned String");
    let borrowed = doc.first_line();
    let owned = borrowed.to_owned();
    println!("borrowed={borrowed:?}"); // borrowed 的最后一次使用。
    doc.replace_text(String::from("TITLE: new\nnew body"));
    println!("owned={owned:?} current={:?}", doc.first_line());

    println!("\n[3] A lifetime describes the relationship between input and output borrows");
    let text = String::from("TITLE: long lived\nbody");
    {
        let fallback = String::from("short lived fallback");
        let preview = choose_preview(&text, &fallback);
        println!("nonempty={preview:?}");
        println!("empty={:?}", choose_preview("", &fallback));
    }

    println!("\n[4] Drop the inner resource at the end of its block");
    let trace = Trace::default();
    cleanup_scope(&trace);
    println!("block: {:?}", trace.events());

    println!("\n[5] Early ? returns release resources; drop(value) can release early");
    for reject in [false, true] {
        let trace = Trace::default();
        let result = early_return(&trace, reject);
        println!("reject={reject} result={result:?} events={:?}", trace.events());
    }
    let trace = Trace::default();
    let resource = TraceResource::open("manual", &trace);
    drop(resource);
    trace.record("after:drop");
    println!("explicit drop: {:?}", trace.events());

    println!("\n[6] Read a real file, drop the File, and return an owned Document");
    let path = std::env::args().nth(1).unwrap_or_else(|| "sample.txt".into());
    let loaded = load_document(&path)?;
    println!("file={path:?} first={:?} (File is already dropped)", loaded.first_line());
    Ok(())
}
