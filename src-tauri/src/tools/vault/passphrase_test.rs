use std::collections::HashSet;

use super::passphrase::{
    MAX_WORDS, MIN_WORDS, NOTES_MIN_WORDS, NOTES_WORD_LIMITS, PassphraseError, PassphraseSession,
    VAULT_WORD_LIMITS, normalize_passphrase, random_below, strength, validate_typed_words,
    validate_typed_words_with,
};
use super::wordlist;

fn generated(count: usize) -> PassphraseSession {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    session.generate(count).unwrap();
    session
}

#[test]
fn generate_returns_requested_count_for_every_valid_count() {
    for count in MIN_WORDS..=MAX_WORDS {
        let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
        let words = session.generate(count).unwrap();
        assert_eq!(words.len(), count);
        assert_eq!(session.word_count(), count);
    }
}

#[test]
fn generate_rejects_invalid_counts() {
    for count in [0, 1, 5, 13, 100] {
        let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
        assert_eq!(
            session.generate(count),
            Err(PassphraseError::WordCountOutOfRange {
                min: MIN_WORDS,
                max: MAX_WORDS
            })
        );
        assert_eq!(session.word_count(), 0);
    }
}

#[test]
fn generated_words_are_in_the_list_and_all_different() {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    let words = session.generate(12).unwrap();
    let unique: HashSet<&String> = words.iter().collect();
    assert_eq!(unique.len(), 12);
    for word in &words {
        assert!(wordlist::contains(word));
    }
}

#[test]
fn two_generations_differ() {
    let mut first = PassphraseSession::new(VAULT_WORD_LIMITS);
    let mut second = PassphraseSession::new(VAULT_WORD_LIMITS);
    assert_ne!(first.generate(10).unwrap(), second.generate(10).unwrap());
}

#[test]
fn refresh_before_generate_fails() {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    assert_eq!(session.refresh(0), Err(PassphraseError::NoPassphrase));
}

#[test]
fn refresh_with_bad_slot_fails() {
    let mut session = generated(6);
    assert_eq!(session.refresh(6), Err(PassphraseError::InvalidWordIndex));
    assert_eq!(session.refresh(99), Err(PassphraseError::InvalidWordIndex));
}

#[test]
fn refresh_changes_only_that_slot_and_returns_the_new_word() {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    let before = session.generate(10).unwrap();
    let new_word = session.refresh(3).unwrap();
    let after = session.display_words();
    assert_eq!(after.len(), 10);
    assert_eq!(after[3], new_word);
    assert_ne!(after[3], before[3]);
    for slot in (0..10).filter(|slot| *slot != 3) {
        assert_eq!(after[slot], before[slot]);
    }
}

#[test]
fn replacement_limit_is_three_per_slot() {
    let mut session = generated(10);
    assert_eq!(session.replacements_left(0), Some(3));
    for expected_left in [2, 1, 0] {
        session.refresh(0).unwrap();
        assert_eq!(session.replacements_left(0), Some(expected_left));
    }
    assert_eq!(
        session.refresh(0),
        Err(PassphraseError::ReplacementLimitReached)
    );
    assert_eq!(session.replacements_left(1), Some(3));
    assert!(session.refresh(1).is_ok());
    assert_eq!(session.replacements_left(99), None);
}

#[test]
fn no_word_is_ever_shown_twice_in_a_session() {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    let mut shown: Vec<String> = session.generate(12).unwrap();
    for slot in 0..12 {
        for _ in 0..3 {
            shown.push(session.refresh(slot).unwrap());
        }
    }
    assert_eq!(shown.len(), 12 + 36);
    let unique: HashSet<&String> = shown.iter().collect();
    assert_eq!(unique.len(), shown.len());
}

#[test]
fn generate_again_resets_the_session() {
    let mut session = generated(10);
    for _ in 0..3 {
        session.refresh(0).unwrap();
    }
    session.confirm_saved().unwrap();
    session.generate(8).unwrap();
    assert_eq!(session.word_count(), 8);
    assert!(!session.is_confirmed());
    assert_eq!(session.replacements_left(0), Some(3));
    assert!(session.refresh(0).is_ok());
}

#[test]
fn kdf_string_requires_confirmation() {
    let mut session = generated(10);
    assert_eq!(
        session.normalized_for_kdf().map(|_| ()),
        Err(PassphraseError::NotConfirmed)
    );
    session.confirm_saved().unwrap();
    let kdf = session.normalized_for_kdf().unwrap();
    assert_eq!(kdf.as_str(), session.display_words().join(" "));
}

#[test]
fn kdf_string_without_passphrase_fails() {
    let mut session = PassphraseSession::new(VAULT_WORD_LIMITS);
    assert_eq!(
        session.normalized_for_kdf().map(|_| ()),
        Err(PassphraseError::NoPassphrase)
    );
    assert_eq!(session.confirm_saved(), Err(PassphraseError::NoPassphrase));
}

#[test]
fn refresh_after_confirmation_is_rejected() {
    let mut session = generated(10);
    session.confirm_saved().unwrap();
    assert_eq!(session.refresh(0), Err(PassphraseError::AlreadyConfirmed));
}

#[test]
fn clear_wipes_the_session() {
    let mut session = generated(10);
    session.confirm_saved().unwrap();
    session.clear();
    assert_eq!(session.word_count(), 0);
    assert!(!session.is_confirmed());
    assert!(session.display_words().is_empty());
}

#[test]
fn normalize_handles_case_spacing_and_hyphens() {
    assert_eq!(
        normalize_passphrase("  Abacus   ABILITY\tAble \n").as_str(),
        "abacus ability able"
    );
    assert_eq!(
        normalize_passphrase("Drop-Down  Yo-Yo").as_str(),
        "drop-down yo-yo"
    );
    assert_eq!(normalize_passphrase("").as_str(), "");
}

#[test]
fn normalize_applies_nfkc() {
    // Full-width letters and a no-break space fold to plain ASCII.
    assert_eq!(
        normalize_passphrase("\u{FF21}\u{FF22}\u{FF23}\u{00A0}abc").as_str(),
        "abc abc"
    );
}

#[test]
fn normalize_is_idempotent() {
    let once = normalize_passphrase("  Abacus   ABILITY\tAble \n");
    let twice = normalize_passphrase(&once);
    assert_eq!(once.as_str(), twice.as_str());
}

#[test]
fn typed_words_in_any_format_match_the_generated_passphrase() {
    let mut session = generated(10);
    session.confirm_saved().unwrap();
    let kdf = session.normalized_for_kdf().unwrap();
    let typed = session
        .display_words()
        .iter()
        .map(|word| word.to_uppercase())
        .collect::<Vec<_>>()
        .join("  ");
    assert_eq!(normalize_passphrase(&typed).as_str(), kdf.as_str());
}

#[test]
fn validate_accepts_good_words_and_names_only_the_position_of_a_bad_one() {
    let list = wordlist::words();
    let good = list[..6].join(" ");
    assert_eq!(validate_typed_words(&good), Ok(()));

    let mut words: Vec<&str> = list[..6].to_vec();
    words[1] = "zzzznotaword";
    assert_eq!(
        validate_typed_words(&words.join(" ")),
        Err(PassphraseError::UnknownWord { position: 2 })
    );
}

#[test]
fn validate_rejects_wrong_word_counts() {
    let list = wordlist::words();
    assert_eq!(
        validate_typed_words(&list[..5].join(" ")),
        Err(PassphraseError::WordCountOutOfRange {
            min: MIN_WORDS,
            max: MAX_WORDS
        })
    );
    assert_eq!(
        validate_typed_words(&list[..13].join(" ")),
        Err(PassphraseError::WordCountOutOfRange {
            min: MIN_WORDS,
            max: MAX_WORDS
        })
    );
    assert_eq!(
        validate_typed_words(""),
        Err(PassphraseError::WordCountOutOfRange {
            min: MIN_WORDS,
            max: MAX_WORDS
        })
    );
    assert_eq!(validate_typed_words(&list[..12].join(" ")), Ok(()));
}

#[test]
fn strength_matches_the_documented_numbers() {
    let ten = strength(10);
    assert!((ten.bits - 129.2).abs() < 0.1);
    assert!((ten.grover_bits - ten.bits / 2.0).abs() < f64::EPSILON);
    assert!(!ten.is_weak);
    assert!(strength(7).is_weak);
    assert!(!strength(8).is_weak);
    assert!((strength(8).bits - 103.4).abs() < 0.1);
}

#[test]
fn random_below_stays_in_range_and_rejects_zero() {
    assert!(random_below(0).is_err());
    assert_eq!(random_below(1).unwrap(), 0);
    for _ in 0..10_000 {
        assert!(random_below(7776).unwrap() < 7776);
    }
}

#[test]
fn random_below_is_roughly_uniform() {
    let mut counts = [0u32; 6];
    for _ in 0..60_000 {
        let value = random_below(6).unwrap();
        counts[value as usize] += 1;
    }
    for count in counts {
        assert!((9_400..=10_600).contains(&count), "bucket count {count}");
    }
}

#[test]
fn default_session_keeps_vault_limits() {
    let mut session = PassphraseSession::default();
    assert_eq!(session.limits(), VAULT_WORD_LIMITS);
    assert_eq!(
        session.generate(5),
        Err(PassphraseError::WordCountOutOfRange { min: 6, max: 12 })
    );
    let words = session.generate(6).unwrap();
    assert_eq!(words.len(), 6);
    let unique: HashSet<&String> = words.iter().collect();
    assert_eq!(unique.len(), 6);
}

#[test]
fn notes_session_accepts_five_to_twelve_words() {
    let mut session = PassphraseSession::new(NOTES_WORD_LIMITS);
    let words = session.generate(5).unwrap();
    assert_eq!(words.len(), 5);
    let unique: HashSet<&String> = words.iter().collect();
    assert_eq!(unique.len(), 5);
    for word in &words {
        assert!(wordlist::contains(word));
    }
    assert_eq!(
        session.generate(4),
        Err(PassphraseError::WordCountOutOfRange { min: 5, max: 12 })
    );
    assert_eq!(
        session.generate(13),
        Err(PassphraseError::WordCountOutOfRange { min: 5, max: 12 })
    );
    assert!(session.generate(12).is_ok());
}

#[test]
fn notes_session_refresh_limit_is_three() {
    let mut session = PassphraseSession::new(NOTES_WORD_LIMITS);
    let words = session.generate(5).unwrap();
    let old = words[0].clone();
    for expected_left in [2, 1, 0] {
        let new_word = session.refresh(0).unwrap();
        assert_ne!(new_word, old);
        assert!(!session.display_words().contains(&old));
        assert_eq!(session.replacements_left(0), Some(expected_left));
    }
    assert_eq!(
        session.refresh(0),
        Err(PassphraseError::ReplacementLimitReached)
    );
    assert!(!session.display_words().contains(&old));
}

#[test]
fn notes_session_clear_keeps_limits() {
    let mut session = PassphraseSession::new(NOTES_WORD_LIMITS);
    session.generate(5).unwrap();
    session.clear();
    assert_eq!(session.limits(), NOTES_WORD_LIMITS);
    assert!(session.generate(5).is_ok());
    assert_eq!(
        session.generate(4),
        Err(PassphraseError::WordCountOutOfRange { min: 5, max: 12 })
    );
}

#[test]
fn validate_typed_words_with_honours_limits() {
    let list = wordlist::words();
    let five = list[..5].join(" ");
    assert_eq!(validate_typed_words_with(&five, NOTES_WORD_LIMITS), Ok(()));
    assert_eq!(
        validate_typed_words_with(&five, VAULT_WORD_LIMITS),
        Err(PassphraseError::WordCountOutOfRange { min: 6, max: 12 })
    );

    let thirteen = list[..13].join(" ");
    assert_eq!(
        validate_typed_words_with(&thirteen, NOTES_WORD_LIMITS),
        Err(PassphraseError::WordCountOutOfRange { min: 5, max: 12 })
    );
    assert_eq!(
        validate_typed_words_with(&thirteen, VAULT_WORD_LIMITS),
        Err(PassphraseError::WordCountOutOfRange { min: 6, max: 12 })
    );

    let mut six: Vec<&str> = list[..6].to_vec();
    six[1] = "zzzznotaword";
    let bad = six.join(" ");
    assert_eq!(
        validate_typed_words_with(&bad, NOTES_WORD_LIMITS),
        Err(PassphraseError::UnknownWord { position: 2 })
    );
    assert_eq!(
        validate_typed_words_with(&bad, VAULT_WORD_LIMITS),
        Err(PassphraseError::UnknownWord { position: 2 })
    );

    assert_eq!(validate_typed_words(&list[..6].join(" ")), Ok(()));
    assert_eq!(
        validate_typed_words(&five),
        Err(PassphraseError::WordCountOutOfRange { min: 6, max: 12 })
    );
}

#[test]
fn word_count_error_text_uses_limits() {
    assert_eq!(
        PassphraseError::WordCountOutOfRange {
            min: MIN_WORDS,
            max: MAX_WORDS
        }
        .to_string(),
        "The passphrase must have between 6 and 12 words."
    );
    assert_eq!(
        PassphraseError::WordCountOutOfRange {
            min: NOTES_MIN_WORDS,
            max: MAX_WORDS
        }
        .to_string(),
        "The passphrase must have between 5 and 12 words."
    );
}

#[test]
fn typed_matches_normalizes_and_compares() {
    let mut session = PassphraseSession::new(NOTES_WORD_LIMITS);
    let words = session.generate(5).unwrap();
    let joined = words.join(" ");
    assert_eq!(
        session.typed_matches(&joined),
        Err(PassphraseError::NotConfirmed)
    );
    session.confirm_saved().unwrap();

    assert_eq!(session.typed_matches(&joined), Ok(true));
    let upper = words
        .iter()
        .map(|word| word.to_uppercase())
        .collect::<Vec<_>>()
        .join("  \t");
    assert_eq!(session.typed_matches(&upper), Ok(true));

    let mut swapped = words.clone();
    swapped.swap(0, 1);
    assert_eq!(session.typed_matches(&swapped.join(" ")), Ok(false));
    assert_eq!(session.typed_matches(&words[..4].join(" ")), Ok(false));

    let empty = PassphraseSession::new(NOTES_WORD_LIMITS);
    assert_eq!(
        empty.typed_matches(&joined),
        Err(PassphraseError::NoPassphrase)
    );
}

#[test]
fn notes_kdf_string_is_stable() {
    let mut session = PassphraseSession::new(NOTES_WORD_LIMITS);
    let words = session.generate(5).unwrap();
    session.confirm_saved().unwrap();
    let joined = words.join(" ");
    let kdf = session.normalized_for_kdf().unwrap();
    assert_eq!(kdf.as_str(), joined);
    let stable = normalize_passphrase(kdf.as_str());
    assert_eq!(stable.as_str(), kdf.as_str());
}

#[test]
fn strength_covers_five_to_twelve_words() {
    let five = strength(5);
    assert!((five.bits - 64.6).abs() < 0.1);
    assert!(five.is_weak);

    let eight = strength(8);
    assert!((eight.bits - 103.4).abs() < 0.1);
    assert!(!eight.is_weak);

    let twelve = strength(12);
    assert!((twelve.bits - 155.1).abs() < 0.1);
}
