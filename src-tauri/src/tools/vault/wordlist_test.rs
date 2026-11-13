use std::collections::HashSet;

use super::wordlist::{self, WORDLIST_LEN};

#[test]
fn wordlist_has_exactly_7776_words() {
    assert_eq!(wordlist::words().len(), WORDLIST_LEN);
}

#[test]
fn wordlist_words_are_unique() {
    let words = wordlist::words();
    let unique: HashSet<&str> = words.iter().copied().collect();
    assert_eq!(unique.len(), words.len());
}

#[test]
fn wordlist_words_are_clean() {
    for word in wordlist::words() {
        assert!(!word.is_empty());
        assert!(
            word.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "unexpected character in a word"
        );
        assert!(!word.starts_with('-'));
        assert!(!word.ends_with('-'));
        assert!(!word.contains("--"));
    }
}

#[test]
fn word_at_and_contains_agree() {
    let first = wordlist::word_at(0).unwrap();
    let last = wordlist::word_at(WORDLIST_LEN - 1).unwrap();
    assert!(wordlist::contains(first));
    assert!(wordlist::contains(last));
    assert!(wordlist::word_at(WORDLIST_LEN).is_none());
    assert!(!wordlist::contains("zzzznotaword"));
    assert!(!wordlist::contains(""));
}
