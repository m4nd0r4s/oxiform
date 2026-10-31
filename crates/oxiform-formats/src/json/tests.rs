use std::borrow::Cow;

use oxiform_core::events::{Event, EventReader, ReaderError, ReaderErrorKind};

use super::JsonReader;

fn events(input: &str) -> Result<Vec<Event<'_>>, ReaderError> {
    let mut reader = JsonReader::new(input, 128);
    let mut events = Vec::new();
    loop {
        let event = reader.next_event()?;
        if event == Event::End {
            return Ok(events);
        }
        events.push(event);
    }
}

fn error(input: &str) -> ReaderError {
    events(input).expect_err("the input should be rejected")
}

fn string(text: &str) -> Event<'_> {
    Event::String(Cow::Borrowed(text))
}

fn key(text: &str) -> Event<'_> {
    Event::Key(Cow::Borrowed(text))
}

#[test]
fn reads_scalars_at_the_top_level() {
    assert_eq!(events("true").unwrap(), [Event::Bool(true)]);
    assert_eq!(events(" false ").unwrap(), [Event::Bool(false)]);
    assert_eq!(events("null").unwrap(), [Event::Null]);
    assert_eq!(events("\"text\"").unwrap(), [string("text")]);
    assert_eq!(events("-12.5e+3").unwrap(), [Event::Number("-12.5e+3")]);
}

#[test]
fn keeps_numbers_as_written() {
    let big = "123456789012345678901234567890";
    assert_eq!(events(big).unwrap(), [Event::Number(big)]);
    assert_eq!(events("0.10").unwrap(), [Event::Number("0.10")]);
}

#[test]
fn reads_nested_containers() {
    let input = r#"{"name": "Rex", "tags": [1, {"a": null}], "empty": {}, "none": []}"#;
    assert_eq!(
        events(input).unwrap(),
        [
            Event::MapStart,
            key("name"),
            string("Rex"),
            key("tags"),
            Event::SequenceStart,
            Event::Number("1"),
            Event::MapStart,
            key("a"),
            Event::Null,
            Event::MapEnd,
            Event::SequenceEnd,
            key("empty"),
            Event::MapStart,
            Event::MapEnd,
            key("none"),
            Event::SequenceStart,
            Event::SequenceEnd,
            Event::MapEnd,
        ]
    );
}

#[test]
fn borrows_strings_without_escapes() {
    let decoded = events("\"plain\"").unwrap();
    assert!(matches!(&decoded[0], Event::String(Cow::Borrowed("plain"))));
}

#[test]
fn decodes_escapes() {
    let decoded = events(r#""a\"b\\c\/d\b\f\n\r\t\u00e9\ud83d\ude00""#).unwrap();
    assert_eq!(
        decoded,
        [Event::String(Cow::Owned(
            "a\"b\\c/d\u{8}\u{c}\n\r\t\u{e9}\u{1f600}".into()
        ))]
    );
}

#[test]
fn keeps_non_ascii_text() {
    assert_eq!(events("\"ação 😀\"").unwrap(), [string("ação 😀")]);
}

#[test]
fn reports_the_end_again_after_the_document() {
    let mut reader = JsonReader::new("1", 128);
    assert_eq!(reader.next_event().unwrap(), Event::Number("1"));
    assert_eq!(reader.next_event().unwrap(), Event::End);
    assert_eq!(reader.next_event().unwrap(), Event::End);
}

#[test]
fn reports_where_each_event_starts() {
    let mut reader = JsonReader::new("[1, \"x\"]", 128);
    let mut offsets = Vec::new();
    while reader.next_event().unwrap() != Event::End {
        offsets.push(reader.offset());
    }
    assert_eq!(offsets, [0, 1, 4, 7]);
}

#[test]
fn rejects_malformed_documents() {
    for input in [
        "",
        "   ",
        "[1,]",
        "{\"a\":1,}",
        "[1 2]",
        "{\"a\" 1}",
        "{1: 2}",
        "{\"a\":}",
        "01",
        "1.",
        "1e",
        "-",
        ".5",
        "+1",
        "tru",
        "nul",
        "[",
        "{",
        "\"open",
        "[1]]",
        "{}x",
        "1 2",
        "'single'",
        "\"bad \\x escape\"",
        "\"\\u12\"",
        "\"tab\tinside\"",
        "\"\\ud800\"",
        "\"\\udc00\"",
        "NaN",
    ] {
        let rejected = error(input);
        assert_eq!(rejected.kind, ReaderErrorKind::Syntax, "{input:?}");
    }
}

#[test]
fn locates_errors_by_line_and_column() {
    let rejected = error("{\n  \"a\": 1,\n  \"b\": ?\n}");
    assert_eq!((rejected.line, rejected.column), (3, 8));
    assert_eq!(rejected.message, "expected a value, found '?'");
}

#[test]
fn counts_columns_in_characters() {
    let rejected = error("[\"é\", ?]");
    assert_eq!((rejected.line, rejected.column), (1, 7));
}

#[test]
fn enforces_the_depth_limit() {
    let mut reader = JsonReader::new("[[[1]]]", 2);
    assert_eq!(reader.next_event().unwrap(), Event::SequenceStart);
    assert_eq!(reader.next_event().unwrap(), Event::SequenceStart);
    let rejected = reader.next_event().unwrap_err();
    assert_eq!(rejected.kind, ReaderErrorKind::DepthLimit { limit: 2 });
    assert!(events(&"[".repeat(100_000)).is_err());
}

#[test]
fn finds_specials_at_every_position_of_a_word() {
    for length in 0..24 {
        let plain = "é".repeat(length / 2) + &"a".repeat(length % 2);
        assert_eq!(
            events(&format!("\"{plain}\"")).unwrap(),
            [Event::String(Cow::Borrowed(plain.as_str()))]
        );
        let escaped = format!("\"{plain}\\n{plain}\"");
        assert_eq!(
            events(&escaped).unwrap(),
            [Event::String(Cow::Owned(format!("{plain}\n{plain}")))]
        );
        let control = format!("\"{plain}\u{1}\"");
        assert_eq!(
            error(&control).message,
            "control characters must be escaped in strings"
        );
    }
}
