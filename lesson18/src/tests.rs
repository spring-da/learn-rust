use super::{Document, choose_preview, first_line, load_document, read_document};
use crate::cleanup::{Trace, TraceResource, cleanup_scope, early_return};
use std::cell::Cell;
use std::io::{self, Cursor, Read};
use std::path::Path;
use std::rc::Rc;

#[test]
fn owned_line_survives_replacement_and_document_drop() {
    let mut doc = Document::new(String::from("TITLE: old\nbody"));
    let owned = doc.first_line().to_owned();
    doc.replace_text(String::from("TITLE: new"));
    assert_eq!(doc.first_line(), "TITLE: new");
    drop(doc);
    assert_eq!(owned, "TITLE: old");
}

#[test]
fn first_line_handles_empty_unicode_and_line_endings() {
    for (input, expected) in [
        ("", ""), ("\nbody", ""), ("one", "one"),
        ("one\ntwo", "one"), ("one\r\ntwo", "one"),
        ("标题：你好\n正文", "标题：你好"),
    ] {
        assert_eq!(first_line(input), expected);
    }
}

#[test]
fn preview_selects_either_input_with_different_actual_lifetimes() {
    let text = String::from("TITLE: actual\nbody");
    {
        let fallback = String::from("fallback");
        assert_eq!(choose_preview(&text, &fallback), "TITLE: actual");
        assert_eq!(choose_preview("\r\nbody", &fallback), "fallback");
    }
    assert_eq!(first_line(&text), "TITLE: actual");
}

#[test]
fn inner_resource_drops_before_outer_body_continues() {
    let trace = Trace::default();
    cleanup_scope(&trace);
    assert_eq!(trace.events(), [
        "open:outer", "open:inner", "body:inner",
        "close:inner", "body:outer", "close:outer",
    ]);
}

#[test]
fn cleanup_runs_on_success_and_early_question_mark_return() {
    for reject in [false, true] {
        let trace = Trace::default();
        let result = early_return(&trace, reject);
        assert_eq!(result.is_err(), reject);
        assert_eq!(trace.events(), [
            "open:validation",
            if reject { "reject" } else { "accept" },
            "close:validation",
        ]);
    }
}

#[test]
fn explicit_drop_releases_once_before_end_of_scope() {
    let trace = Trace::default();
    {
        let resource = TraceResource::open("manual", &trace);
        drop(resource);
        trace.record("after:drop");
    }
    assert_eq!(trace.events(), ["open:manual", "close:manual", "after:drop"]);
}

struct SpyReader {
    data: Cursor<Vec<u8>>,
    fail_read: bool,
    drops: Rc<Cell<usize>>,
}

impl Read for SpyReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.fail_read {
            return Err(io::Error::other("injected read failure"));
        }
        self.data.read(buffer)
    }
}

impl Drop for SpyReader {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn owned_reader_drops_exactly_once_on_every_return_path() {
    let valid = "TITLE: 文档\r\nbody".as_bytes();
    let cases: [(&[u8], bool, Option<io::ErrorKind>); 4] = [
        (valid, false, None),
        (b"body", false, Some(io::ErrorKind::InvalidData)),
        (&[0xff], false, Some(io::ErrorKind::InvalidData)),
        (valid, true, Some(io::ErrorKind::Other)),
    ];
    for (data, fail_read, expected_error) in cases {
        let drops = Rc::new(Cell::new(0));
        let reader = SpyReader {
            data: Cursor::new(data.to_vec()),
            fail_read,
            drops: drops.clone(),
        };
        let result = read_document(reader);
        assert_eq!(drops.get(), 1);
        match expected_error {
            Some(kind) => assert_eq!(result.unwrap_err().kind(), kind),
            None => assert_eq!(result.unwrap().first_line(), "TITLE: 文档"),
        }
    }
}

#[test]
fn passing_mutable_reader_reference_keeps_owner_alive() {
    let drops = Rc::new(Cell::new(0));
    let mut reader = SpyReader {
        data: Cursor::new(b"TITLE: borrowed reader".to_vec()),
        fail_read: false,
        drops: drops.clone(),
    };
    let doc = read_document(&mut reader).unwrap();
    assert_eq!(doc.first_line(), "TITLE: borrowed reader");
    assert_eq!(drops.get(), 0);
    drop(reader);
    assert_eq!(drops.get(), 1);
}

#[test]
fn file_loader_returns_text_and_reports_missing_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = load_document(root.join("sample.txt")).unwrap();
    assert_eq!(doc.first_line(), "TITLE: Go 与 Rust 的文档预览");
    let error = load_document(root.join("sample.txt.missing")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}
