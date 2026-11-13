//! The embedded EFF large wordlist (7776 words).
//!
//! Source: <https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt>
//! (dice numbers stripped, one lowercase word per line).
//! License: Creative Commons Attribution 3.0 United States (CC BY 3.0 US),
//! (c) Electronic Frontier Foundation. The attribution must be shown in
//! the app's About screen.
//!
//! The list is compiled into the binary, so there is no file access at
//! run time and nothing to tamper with on disk.

use std::sync::OnceLock;

/// Raw list text, embedded at compile time.
const RAW_WORDLIST: &str = include_str!("eff_large_wordlist.txt");

/// Exact number of words the list must have (6^5 dice combinations).
/// The entropy math (log2(7776) bits per word) depends on this number.
pub const WORDLIST_LEN: usize = 7776;

static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();

/// All words, parsed once on first use.
pub fn words() -> &'static [&'static str] {
    WORDS
        .get_or_init(|| {
            RAW_WORDLIST
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect()
        })
        .as_slice()
}

/// The word at `index`, or `None` when the index is out of range.
pub fn word_at(index: usize) -> Option<&'static str> {
    words().get(index).copied()
}

/// True when `word` is exactly one of the list's words.
pub fn contains(word: &str) -> bool {
    words().contains(&word)
}
