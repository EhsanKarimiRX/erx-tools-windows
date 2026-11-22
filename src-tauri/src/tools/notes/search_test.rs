//! Tests for the title search and its letter normalization.

use super::model::Note;
use super::search::*;

fn note_at(index: usize, title: &str, body: &str) -> Note {
    Note::new(
        format!("{index:032x}"),
        title.to_string(),
        body.to_string(),
        0,
    )
}

fn notes_with_titles(titles: &[&str]) -> Vec<Note> {
    titles
        .iter()
        .enumerate()
        .map(|(i, title)| note_at(i, title, "body"))
        .collect()
}

#[test]
fn s1_normalize_for_search() {
    assert_eq!(
        normalize_for_search("\u{064A}"),
        normalize_for_search("\u{06CC}")
    );
    assert_eq!(
        normalize_for_search("\u{0643}"),
        normalize_for_search("\u{06A9}")
    );
    assert_eq!(normalize_for_search("HeLLo"), "hello");

    let half_space = normalize_for_search("a\u{200C}b");
    assert!(half_space.contains('\u{200C}'));

    assert_eq!(
        normalize_for_search("\u{06F1}\u{06F2}\u{06F3}"),
        "\u{06F1}\u{06F2}\u{06F3}"
    );
    assert_eq!(
        normalize_for_search("\u{FEF1}"),
        normalize_for_search("\u{06CC}")
    );
}

#[test]
fn s2_search_titles() {
    let notes = notes_with_titles(&["alpha", "beta", "gamma"]);

    assert_eq!(search_titles(&notes, ""), vec![0, 1, 2]);
    assert_eq!(search_titles(&notes, "   "), vec![0, 1, 2]);

    let yeh = notes_with_titles(&["\u{0639}\u{0644}\u{064A}"]);
    assert_eq!(search_titles(&yeh, "\u{0639}\u{0644}\u{06CC}"), vec![0]);
    let persian_yeh = notes_with_titles(&["\u{0639}\u{0644}\u{06CC}"]);
    assert_eq!(
        search_titles(&persian_yeh, "\u{0639}\u{0644}\u{064A}"),
        vec![0]
    );

    let kaf = notes_with_titles(&["\u{0643}\u{062A}\u{0627}\u{0628}"]);
    assert_eq!(
        search_titles(&kaf, "\u{06A9}\u{062A}\u{0627}\u{0628}"),
        vec![0]
    );

    let latin = notes_with_titles(&["Hello World"]);
    assert_eq!(search_titles(&latin, "hELLo"), vec![0]);

    let half_space = notes_with_titles(&["a\u{200C}b"]);
    assert_eq!(search_titles(&half_space, "a\u{200C}b"), vec![0]);

    assert_eq!(search_titles(&latin, "  hello  "), vec![0]);

    let body_only = notes_with_titles(&["alpha"]);
    let mut body_only = body_only;
    body_only[0].body = "needle".to_string();
    assert!(search_titles(&body_only, "needle").is_empty());

    assert!(search_titles(&notes, "zzz").is_empty());

    let order = notes_with_titles(&["ant", "bee", "cat", "dog", "ear"]);
    assert_eq!(search_titles(&order, "a"), vec![0, 2, 4]);
}
