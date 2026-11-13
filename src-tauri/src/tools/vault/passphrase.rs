//! Passphrase generation, session state and normalization.
//!
//! SECURITY-CRITICAL MODULE. The passphrase lives in Rust only. React
//! receives words for DISPLAY and never sends them back for encryption.
//! The session stores word INDICES (not strings) and wipes them on
//! `clear()` and on drop. Rules implemented here:
//! - words are drawn with OS randomness, uniformly, without bias;
//! - words in one passphrase are all different;
//! - each word slot may be replaced at most 3 times (counter kept here);
//! - a replaced word is gone for good and no word is shown twice in one
//!   session (a session starts at every `generate`);
//! - after the user confirms "I saved it" the passphrase is frozen;
//! - encryption and decryption share ONE normalization function, so the
//!   same words always give the same bytes for the key derivation.

use unicode_normalization::UnicodeNormalization;
use zeroize::{Zeroize, Zeroizing};

use super::wordlist::{self, WORDLIST_LEN};

/// Fewest words a passphrase may have.
pub const MIN_WORDS: usize = 6;
/// Most words a passphrase may have.
pub const MAX_WORDS: usize = 12;
/// Default word count offered by the UI.
pub const DEFAULT_WORDS: usize = 10;
/// Word counts below this value get a "weak" warning.
pub const WEAK_BELOW_WORDS: usize = 8;
/// How many times one word slot may be replaced.
pub const MAX_REPLACEMENTS_PER_WORD: u8 = 3;

/// Safety cap for the "pick an unseen word" retry loop.
const MAX_PICK_ATTEMPTS: usize = 1000;
/// Words are joined with exactly one of these in the normalized form.
const WORD_SEPARATOR: char = ' ';

/// Errors of the passphrase module. Messages are shown to the user, so they
/// are plain English and never contain the words themselves.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PassphraseError {
    #[error("The passphrase must have between 6 and 12 words.")]
    WordCountOutOfRange,
    #[error("Windows could not provide secure random numbers.")]
    RandomUnavailable,
    #[error("There is no passphrase yet. Generate one first.")]
    NoPassphrase,
    #[error("That word position does not exist.")]
    InvalidWordIndex,
    #[error("This word has already been replaced 3 times.")]
    ReplacementLimitReached,
    #[error("No unused words are left to choose from.")]
    NoUnusedWords,
    #[error("The passphrase is already confirmed and can no longer be changed.")]
    AlreadyConfirmed,
    #[error("Please confirm that you saved the passphrase first.")]
    NotConfirmed,
    #[error("Word number {position} is not in the word list. Check the spelling.")]
    UnknownWord { position: usize },
    #[error("The built-in word list is damaged. Reinstall the app.")]
    WordlistDamaged,
}

/// The word list, but only if it has exactly the expected size. A damaged
/// list would silently weaken every passphrase, so it is refused.
fn checked_words() -> Result<&'static [&'static str], PassphraseError> {
    let words = wordlist::words();
    if words.len() != WORDLIST_LEN {
        return Err(PassphraseError::WordlistDamaged);
    }
    Ok(words)
}

/// Uniform random number in `0..bound` from OS randomness.
///
/// SECURITY: plain `value % bound` would favour small results (modulo
/// bias). Instead every draw at or above the largest multiple of `bound`
/// that fits in a u32 is thrown away, so all results are equally likely.
pub(super) fn random_below(bound: u32) -> Result<u32, PassphraseError> {
    if bound == 0 {
        return Err(PassphraseError::NoUnusedWords);
    }
    let limit = (u32::MAX / bound) * bound;
    loop {
        let value = getrandom::u32().map_err(|_| PassphraseError::RandomUnavailable)?;
        if value < limit {
            return Ok(value % bound);
        }
    }
}

/// A random word index that is not in `seen`.
fn pick_unseen(list_len: usize, seen: &[usize]) -> Result<usize, PassphraseError> {
    if seen.len() >= list_len {
        return Err(PassphraseError::NoUnusedWords);
    }
    let bound = u32::try_from(list_len).map_err(|_| PassphraseError::WordlistDamaged)?;
    for _ in 0..MAX_PICK_ATTEMPTS {
        let candidate = random_below(bound)? as usize;
        if !seen.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(PassphraseError::NoUnusedWords)
}

/// Normalizes a passphrase for the key derivation: Unicode NFKC, lowercase,
/// NFKC again, then words split on any whitespace and joined with exactly
/// one space.
///
/// SECURITY: encryption and decryption MUST both use this function, and
/// nothing else, to turn words into the bytes fed to Argon2id. Buffers are
/// pre-sized and wrapped in `Zeroizing` so copies are wiped on drop
/// (best effort: Unicode expansion in the first step may still reallocate).
pub fn normalize_passphrase(input: &str) -> Zeroizing<String> {
    let mut composed = Zeroizing::new(String::with_capacity(input.len()));
    composed.extend(input.nfkc());
    let lowered = Zeroizing::new(composed.to_lowercase());
    let mut recomposed = Zeroizing::new(String::with_capacity(lowered.len()));
    recomposed.extend(lowered.as_str().nfkc());
    let mut normalized = Zeroizing::new(String::with_capacity(recomposed.len()));
    for (position, word) in recomposed.split_whitespace().enumerate() {
        if position > 0 {
            normalized.push(WORD_SEPARATOR);
        }
        normalized.push_str(word);
    }
    normalized
}

/// Checks words typed by the user (already passed through
/// `normalize_passphrase`): the count must be 6..=12 and every word must be
/// in the list. The error names only the word's POSITION, never the word.
pub fn validate_typed_words(normalized: &str) -> Result<(), PassphraseError> {
    let count = normalized
        .split(WORD_SEPARATOR)
        .filter(|word| !word.is_empty())
        .count();
    if !(MIN_WORDS..=MAX_WORDS).contains(&count) {
        return Err(PassphraseError::WordCountOutOfRange);
    }
    for (position, word) in normalized.split(WORD_SEPARATOR).enumerate() {
        if !wordlist::contains(word) {
            return Err(PassphraseError::UnknownWord {
                position: position + 1,
            });
        }
    }
    Ok(())
}

/// How strong a passphrase of `word_count` words is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassphraseStrength {
    /// Classical entropy in bits: word_count * log2(7776).
    pub bits: f64,
    /// Rough security against Grover's quantum search: half of `bits`.
    pub grover_bits: f64,
    /// True when the word count is below the recommended minimum.
    pub is_weak: bool,
}

/// Entropy figures for the UI meter (10 words is about 129 bits).
pub fn strength(word_count: usize) -> PassphraseStrength {
    let bits = word_count as f64 * (WORDLIST_LEN as f64).log2();
    PassphraseStrength {
        bits,
        grover_bits: bits / 2.0,
        is_weak: word_count < WEAK_BELOW_WORDS,
    }
}

/// The state of one passphrase-generation session. Lives in Rust state.
#[derive(Default)]
pub struct PassphraseSession {
    /// Word-list indices of the current passphrase, in slot order.
    slots: Vec<usize>,
    /// How many times each slot has been replaced (same length as `slots`).
    replacements: Vec<u8>,
    /// Every word index shown since `generate`; none is ever shown again.
    seen: Vec<usize>,
    /// True once the user confirmed "I saved it".
    confirmed: bool,
}

impl PassphraseSession {
    /// An empty session (no passphrase yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new session with `count` fresh, different words. Discards
    /// any previous passphrase and resets all counters and the confirmation.
    pub fn generate(&mut self, count: usize) -> Result<Vec<String>, PassphraseError> {
        if !(MIN_WORDS..=MAX_WORDS).contains(&count) {
            return Err(PassphraseError::WordCountOutOfRange);
        }
        let list = checked_words()?;
        self.clear();
        for _ in 0..count {
            match pick_unseen(list.len(), &self.seen) {
                Ok(index) => {
                    self.slots.push(index);
                    self.seen.push(index);
                }
                Err(error) => {
                    self.clear();
                    return Err(error);
                }
            }
        }
        self.replacements = vec![0; count];
        Ok(self.display_words())
    }

    /// The current words in slot order, for DISPLAY only.
    pub fn display_words(&self) -> Vec<String> {
        self.slots
            .iter()
            .filter_map(|&index| wordlist::word_at(index))
            .map(str::to_owned)
            .collect()
    }

    /// Replaces the word in `slot` with a never-shown word and returns it.
    /// One-way: the old word is never restored or shown again.
    pub fn refresh(&mut self, slot: usize) -> Result<String, PassphraseError> {
        if self.slots.is_empty() {
            return Err(PassphraseError::NoPassphrase);
        }
        if self.confirmed {
            return Err(PassphraseError::AlreadyConfirmed);
        }
        let used = *self
            .replacements
            .get(slot)
            .ok_or(PassphraseError::InvalidWordIndex)?;
        if used >= MAX_REPLACEMENTS_PER_WORD {
            return Err(PassphraseError::ReplacementLimitReached);
        }
        let list = checked_words()?;
        let new_index = pick_unseen(list.len(), &self.seen)?;
        let word = wordlist::word_at(new_index).ok_or(PassphraseError::WordlistDamaged)?;
        // All checks passed: commit the change.
        let (Some(entry), Some(counter)) =
            (self.slots.get_mut(slot), self.replacements.get_mut(slot))
        else {
            return Err(PassphraseError::InvalidWordIndex);
        };
        *entry = new_index;
        *counter = counter.saturating_add(1);
        self.seen.push(new_index);
        Ok(word.to_owned())
    }

    /// Replacements still available for `slot` (None for a bad slot).
    pub fn replacements_left(&self, slot: usize) -> Option<u8> {
        self.replacements
            .get(slot)
            .map(|used| MAX_REPLACEMENTS_PER_WORD.saturating_sub(*used))
    }

    /// Number of words in the current passphrase (0 when none).
    pub fn word_count(&self) -> usize {
        self.slots.len()
    }

    /// Records the user's "I saved it" confirmation and freezes the words.
    pub fn confirm_saved(&mut self) -> Result<(), PassphraseError> {
        if self.slots.is_empty() {
            return Err(PassphraseError::NoPassphrase);
        }
        self.confirmed = true;
        Ok(())
    }

    /// True after `confirm_saved`.
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// The normalized passphrase for the key derivation. Refused until the
    /// user has confirmed that the passphrase was saved.
    pub fn normalized_for_kdf(&self) -> Result<Zeroizing<String>, PassphraseError> {
        if self.slots.is_empty() {
            return Err(PassphraseError::NoPassphrase);
        }
        if !self.confirmed {
            return Err(PassphraseError::NotConfirmed);
        }
        // Pre-sized (12 words * 16 bytes is more than enough) so the buffer
        // never reallocates and leaves an unwiped copy behind.
        let mut joined = Zeroizing::new(String::with_capacity(MAX_WORDS * 16));
        for (position, &index) in self.slots.iter().enumerate() {
            let word = wordlist::word_at(index).ok_or(PassphraseError::WordlistDamaged)?;
            if position > 0 {
                joined.push(WORD_SEPARATOR);
            }
            joined.push_str(word);
        }
        Ok(normalize_passphrase(&joined))
    }

    /// Wipes the passphrase and resets the session.
    pub fn clear(&mut self) {
        self.slots.zeroize();
        self.replacements.zeroize();
        self.seen.zeroize();
        self.confirmed = false;
    }
}

impl Drop for PassphraseSession {
    fn drop(&mut self) {
        self.clear();
    }
}
