use oxiform_core::events::{Event, EventReader, ReaderErrorKind};
use oxiform_core::limits::Limits;

use super::PairsBuilder;

/// The events of a query string as `(kind, text)` strings, or the fault that stopped them.
fn events(
    input: &str,
    limits: &Limits,
    fold_case: bool,
) -> Result<Vec<String>, (ReaderErrorKind, String)> {
    let mut builder = PairsBuilder::new(limits, fold_case);
    builder.add_urlencoded(input.as_bytes());
    let mut reader = builder.finish();
    let mut seen = Vec::new();
    loop {
        match reader.next_event() {
            Ok(Event::End) => return Ok(seen),
            Ok(Event::MapStart) => seen.push("{".into()),
            Ok(Event::MapEnd) => seen.push("}".into()),
            Ok(Event::SequenceStart) => seen.push("[".into()),
            Ok(Event::SequenceEnd) => seen.push("]".into()),
            Ok(Event::Key(key)) => seen.push(format!("key {key}")),
            Ok(Event::String(text)) => seen.push(format!("str {text}")),
            Ok(other) => panic!("unexpected {other:?}"),
            Err(error) => return Err((error.kind.clone(), error.message.clone())),
        }
    }
}

fn read(input: &str) -> Vec<String> {
    events(input, &Limits::DEFAULT, false).unwrap()
}

#[test]
fn groups_the_values_of_a_name_in_order_of_first_appearance() {
    assert_eq!(
        read("a=1&b=2&a=3"),
        [
            "{", "key a", "[", "str 1", "str 3", "]", "key b", "[", "str 2", "]", "}"
        ]
    );
}

#[test]
fn a_name_without_an_equals_sign_or_a_value_holds_the_empty_string() {
    assert_eq!(
        read("flag&x="),
        [
            "{", "key flag", "[", "str ", "]", "key x", "[", "str ", "]", "}"
        ]
    );
}

#[test]
fn empty_segments_are_skipped() {
    assert_eq!(read("&&a=1&&"), ["{", "key a", "[", "str 1", "]", "}"]);
    assert_eq!(read(""), ["{", "}"]);
}

#[test]
fn only_the_first_equals_sign_splits() {
    assert_eq!(read("a=b=c"), ["{", "key a", "[", "str b=c", "]", "}"]);
}

#[test]
fn a_plus_is_a_space_and_an_escaped_plus_is_a_plus() {
    assert_eq!(
        read("q=a+b%2Bc%20d"),
        ["{", "key q", "[", "str a b+c d", "]", "}"]
    );
}

#[test]
fn names_are_decoded_too() {
    assert_eq!(
        read("page%5Fsize=1"),
        ["{", "key page_size", "[", "str 1", "]", "}"]
    );
}

#[test]
fn percent_escapes_decode_to_utf8() {
    assert_eq!(
        read("n=%C3%A9%e2%82%AC"),
        ["{", "key n", "[", "str é€", "]", "}"]
    );
}

#[test]
fn an_invalid_escape_is_a_syntax_fault_after_the_pairs_before_it() {
    let fault = events("a=1&b=%zz", &Limits::DEFAULT, false).unwrap_err();
    assert_eq!(fault.0, ReaderErrorKind::Syntax);
    assert!(fault.1.contains("%zz"), "{}", fault.1);
    assert!(events("a=%4", &Limits::DEFAULT, false).is_err());
    assert!(events("a=%", &Limits::DEFAULT, false).is_err());
    assert!(events("%G0=a", &Limits::DEFAULT, false).is_err());
}

#[test]
fn bytes_that_are_not_utf8_are_a_syntax_fault() {
    for input in ["a=%ff", "a=%C3", "%80=a", "a=%C3%28"] {
        let fault = events(input, &Limits::DEFAULT, false).unwrap_err();
        assert_eq!(fault.0, ReaderErrorKind::Syntax, "{input}");
        assert!(fault.1.contains("UTF-8"), "{}", fault.1);
    }
}

#[test]
fn names_that_differ_in_case_are_one_name_when_folding() {
    let folded = events("Accept=a&accept=b&ACCEPT=c", &Limits::DEFAULT, true).unwrap();
    assert_eq!(
        folded,
        ["{", "key Accept", "[", "str a", "str b", "str c", "]", "}"]
    );
    assert_eq!(
        read("Accept=a&accept=b")
            .iter()
            .filter(|e| e.starts_with("key"))
            .count(),
        2
    );
}

#[test]
fn a_pair_list_is_not_percent_decoded() {
    let mut builder = PairsBuilder::new(&Limits::DEFAULT, true);
    builder.add_pair(b"Cookie", b"a=%41+b");
    builder.add_pair("X-Name".as_bytes(), "é".as_bytes());
    let mut reader = builder.finish();
    let mut texts = Vec::new();
    loop {
        match reader.next_event().unwrap() {
            Event::End => break,
            Event::String(text) => texts.push(text.into_owned()),
            _ => {}
        }
    }
    assert_eq!(texts, ["a=%41+b", "é"]);
}

#[test]
fn a_pair_that_is_not_utf8_is_a_syntax_fault() {
    let mut builder = PairsBuilder::new(&Limits::DEFAULT, false);
    builder.add_pair(b"x", b"\xff");
    let mut reader = builder.finish();
    assert_eq!(reader.next_event(), Ok(Event::MapStart));
    assert_eq!(reader.next_event(), Ok(Event::Key("x".into())));
    assert_eq!(reader.next_event(), Ok(Event::SequenceStart));
    let fault = reader.next_event().unwrap_err();
    assert_eq!(fault.kind, ReaderErrorKind::Syntax);
    assert!(fault.message.contains("value"), "{}", fault.message);
}

#[test]
fn limits_are_enforced() {
    let limits = Limits {
        max_bytes: 20,
        max_items: 3,
        max_str_len: 4,
        ..Limits::DEFAULT
    };
    let kind = |input| events(input, &limits, false).unwrap_err().0;
    assert_eq!(
        kind("a=1&b=2&c=3&d=4"),
        ReaderErrorKind::ItemLimit { limit: 3 }
    );
    assert_eq!(kind("a=12345"), ReaderErrorKind::StringLimit { limit: 4 });
    assert_eq!(kind("abcde=1"), ReaderErrorKind::StringLimit { limit: 4 });
    assert_eq!(
        kind("a=1&b=2&c=3&&&&&&&&&&&&"),
        ReaderErrorKind::ByteLimit { limit: 20 }
    );
    assert!(events("a=1&b=2&c=3", &limits, false).is_ok());
}
