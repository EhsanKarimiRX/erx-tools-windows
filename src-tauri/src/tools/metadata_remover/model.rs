//! Data types shared by all handlers of the Metadata Remover.
//!
//! A handler reads one file and returns an `Analysis`: what hidden data it
//! found (`findings`), what it deliberately keeps (`kept`) and a `Plan`, the
//! exact recipe for the cleaned file (copy these byte ranges, insert these
//! bytes, write these zeros). The executor turns a plan into a file. Because
//! scanning and cleaning use the same plan, what the user is shown is what
//! is removed.

use serde::Serialize;

/// Largest raw EXIF or XMP block a handler keeps in memory for the "found"
/// list (1 MiB). Bigger blocks are still removed, only not read.
pub const MAX_RAW_METADATA: usize = 1024 * 1024;

/// The file formats this tool can clean (always detected from content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileFormat {
    Jpeg,
    Png,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Heic,
    Mp4,
    Mov,
    Mkv,
}

impl FileFormat {
    /// Short display name used in the UI.
    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Jpeg => "JPEG",
            FileFormat::Png => "PNG",
            FileFormat::Webp => "WebP",
            FileFormat::Gif => "GIF",
            FileFormat::Bmp => "BMP",
            FileFormat::Tiff => "TIFF",
            FileFormat::Heic => "HEIC",
            FileFormat::Mp4 => "MP4",
            FileFormat::Mov => "MOV",
            FileFormat::Mkv => "MKV",
        }
    }

    pub fn is_video(self) -> bool {
        matches!(self, FileFormat::Mp4 | FileFormat::Mov | FileFormat::Mkv)
    }
}

/// Group of one finding, used by the UI to sort and label them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Location,
    Camera,
    Date,
    Software,
    Author,
    Comment,
    Thumbnail,
    Xmp,
    Iptc,
    Other,
    ExtraData,
}

/// One kind of hidden data found in a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub category: Category,
    pub label: String,
    pub detail: Option<String>,
}

impl Finding {
    pub fn new(category: Category, label: &str) -> Self {
        Finding {
            category,
            label: label.to_string(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanOptions {
    /// Keep the photo rotation (a minimal block with only the orientation).
    pub keep_orientation: bool,
    /// Keep the embedded color profile (ICC).
    pub keep_color_profile: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        CleanOptions {
            keep_orientation: true,
            keep_color_profile: true,
        }
    }
}

/// One step of the recipe for the cleaned file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Copy `len` bytes of the source file, starting at `start`.
    Copy { start: u64, len: u64 },
    /// Write these bytes.
    Insert(Vec<u8>),
    /// Write `n` zero bytes.
    Zeros(u64),
}

/// The ordered recipe for the cleaned file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub ops: Vec<Op>,
}

impl Plan {
    pub fn new() -> Self {
        Plan::default()
    }

    /// Adds a copy step. Empty copies are ignored; a copy that continues
    /// exactly where the previous copy ended is merged into it.
    pub fn copy(&mut self, start: u64, len: u64) {
        if len == 0 {
            return;
        }
        if let Some(Op::Copy {
            start: last_start,
            len: last_len,
        }) = self.ops.last_mut()
            && last_start.checked_add(*last_len) == Some(start)
        {
            *last_len = last_len.saturating_add(len);
            return;
        }
        self.ops.push(Op::Copy { start, len });
    }

    /// Adds bytes to write. Empty inserts are ignored.
    pub fn insert(&mut self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.ops.push(Op::Insert(bytes));
        }
    }

    /// Adds a run of zero bytes. A run of length 0 is ignored.
    pub fn zeros(&mut self, count: u64) {
        if count > 0 {
            self.ops.push(Op::Zeros(count));
        }
    }

    /// Size of the file this plan produces.
    pub fn output_len(&self) -> u64 {
        self.ops.iter().fold(0u64, |total, op| {
            let part = match op {
                Op::Copy { len, .. } => *len,
                Op::Insert(bytes) => bytes.len() as u64,
                Op::Zeros(count) => *count,
            };
            total.saturating_add(part)
        })
    }

    /// True when the plan reproduces the source file byte for byte.
    pub fn is_identity(&self, source_len: u64) -> bool {
        if source_len == 0 {
            return self.ops.is_empty();
        }
        matches!(self.ops.as_slice(), [Op::Copy { start: 0, len }] if *len == source_len)
    }
}

/// Everything a handler learned about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analysis {
    pub format: FileFormat,
    pub source_len: u64,
    /// Hidden data that the plan removes.
    pub findings: Vec<Finding>,
    /// Things that were looked at and deliberately kept.
    pub kept: Vec<String>,
    pub plan: Plan,
    /// Raw EXIF (TIFF structure, without the "Exif" prefix), if present.
    pub exif: Option<Vec<u8>>,
    /// Raw XMP packet, if present.
    pub xmp: Option<Vec<u8>>,
}

impl Analysis {
    pub fn new(format: FileFormat, source_len: u64) -> Self {
        Analysis {
            format,
            source_len,
            findings: Vec::new(),
            kept: Vec::new(),
            plan: Plan::new(),
            exif: None,
            xmp: None,
        }
    }

    /// Adds a finding unless an identical one is already listed.
    pub fn add_finding(&mut self, finding: Finding) {
        if !self.findings.contains(&finding) {
            self.findings.push(finding);
        }
    }

    /// Adds a note about something kept, once.
    pub fn add_kept(&mut self, label: &str) {
        if !self.kept.iter().any(|note| note.as_str() == label) {
            self.kept.push(label.to_string());
        }
    }

    /// True when the cleaned file would differ from the source.
    pub fn has_changes(&self) -> bool {
        !self.plan.is_identity(self.source_len)
    }
}

/// Test helper: a tiny deterministic pseudo-random generator (xorshift64).
#[cfg(test)]
pub struct TestRng(u64);

#[cfg(test)]
impl TestRng {
    pub fn new(seed: u64) -> Self {
        TestRng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// Test helper: every copy step must lie inside the source file and the
/// copy steps must go forward without overlapping.
#[cfg(test)]
pub fn assert_plan_sane(analysis: &Analysis) {
    let mut last_end = 0u64;
    for op in &analysis.plan.ops {
        if let Op::Copy { start, len } = op {
            assert!(*start >= last_end, "copy ranges overlap or run backwards");
            let end = start.checked_add(*len).expect("copy range overflows");
            assert!(
                end <= analysis.source_len,
                "copy range reaches beyond the file"
            );
            last_end = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_next_to_each_other_are_merged() {
        let mut plan = Plan::new();
        plan.copy(0, 3);
        plan.copy(3, 2);
        assert_eq!(plan.ops, vec![Op::Copy { start: 0, len: 5 }]);
        plan.copy(6, 1);
        assert_eq!(plan.ops.len(), 2);
        plan.copy(7, 1);
        assert_eq!(
            plan.ops,
            vec![Op::Copy { start: 0, len: 5 }, Op::Copy { start: 6, len: 2 }]
        );
        plan.copy(20, 0);
        assert_eq!(plan.ops.len(), 2);
    }

    #[test]
    fn inserts_and_zeros_break_copy_runs() {
        let mut plan = Plan::new();
        plan.copy(0, 2);
        plan.insert(Vec::new());
        plan.copy(2, 2);
        plan.insert(vec![1, 2]);
        plan.copy(4, 1);
        plan.zeros(0);
        plan.zeros(3);
        assert_eq!(
            plan.ops,
            vec![
                Op::Copy { start: 0, len: 4 },
                Op::Insert(vec![1, 2]),
                Op::Copy { start: 4, len: 1 },
                Op::Zeros(3)
            ]
        );
        assert_eq!(plan.output_len(), 10);
    }

    #[test]
    fn identity_detection() {
        let mut plan = Plan::new();
        plan.copy(0, 10);
        assert!(plan.is_identity(10));
        assert!(!plan.is_identity(11));
        assert!(Plan::new().is_identity(0));
        assert!(!Plan::new().is_identity(5));

        let mut cut = Plan::new();
        cut.copy(0, 5);
        cut.copy(6, 4);
        assert!(!cut.is_identity(10));

        let mut inserted = Plan::new();
        inserted.copy(0, 10);
        inserted.insert(vec![0]);
        assert!(!inserted.is_identity(10));
    }

    #[test]
    fn analysis_tracks_changes_and_ignores_duplicate_notes() {
        let mut analysis = Analysis::new(FileFormat::Jpeg, 10);
        analysis.plan.copy(0, 10);
        assert!(!analysis.has_changes());
        analysis.add_finding(Finding::new(Category::Comment, "JPEG comment"));
        analysis.add_finding(Finding::new(Category::Comment, "JPEG comment"));
        analysis.add_kept("JFIF header");
        analysis.add_kept("JFIF header");
        assert_eq!(analysis.findings.len(), 1);
        assert_eq!(analysis.kept, vec!["JFIF header".to_string()]);
        analysis.plan.zeros(1);
        assert!(analysis.has_changes());
    }

    #[test]
    fn findings_serialize_with_camel_case_names() {
        let finding = Finding::new(Category::Location, "GPS location");
        assert_eq!(
            serde_json::to_string(&finding).unwrap(),
            r#"{"category":"location","label":"GPS location","detail":null}"#
        );
        let finding = Finding::new(Category::ExtraData, "Extra data").with_detail("5 bytes");
        assert_eq!(
            serde_json::to_string(&finding).unwrap(),
            r#"{"category":"extraData","label":"Extra data","detail":"5 bytes"}"#
        );
        assert_eq!(
            serde_json::to_string(&FileFormat::Webp).unwrap(),
            "\"webp\""
        );
    }

    #[test]
    fn labels_and_defaults() {
        assert_eq!(FileFormat::Jpeg.label(), "JPEG");
        assert_eq!(FileFormat::Mov.label(), "MOV");
        assert!(FileFormat::Mp4.is_video());
        assert!(FileFormat::Mov.is_video());
        assert!(!FileFormat::Heic.is_video());
        let options = CleanOptions::default();
        assert!(options.keep_orientation);
        assert!(options.keep_color_profile);
    }
}
