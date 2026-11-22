//! Title search for Secure Notes, tolerant of Arabic and Persian letter forms.

use unicode_normalization::UnicodeNormalization;

use super::model::Note;

/// Normalizes text for search: NFKC, Arabic yeh/kaf to Persian, lower-case.
pub fn normalize_for_search(text: &str) -> String {
    let normalized: String = text.nfkc().collect();
    normalized
        .replace('\u{064A}', "\u{06CC}")
        .replace('\u{0643}', "\u{06A9}")
        .to_lowercase()
}

/// Indices of notes whose title contains the query, in original order.
pub fn search_titles(notes: &[Note], query: &str) -> Vec<usize> {
    let query = normalize_for_search(query.trim());
    if query.is_empty() {
        return (0..notes.len()).collect();
    }
    notes
        .iter()
        .enumerate()
        .filter(|(_, note)| normalize_for_search(&note.title).contains(&query))
        .map(|(index, _)| index)
        .collect()
}
