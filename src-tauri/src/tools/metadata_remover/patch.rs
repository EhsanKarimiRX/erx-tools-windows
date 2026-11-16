//! Replacing byte ranges of a file IN PLACE: every patch replaces the range
//! [start, end) by some bytes (`head`) followed by zeros up to the old size.
//! The file therefore keeps its size and every offset stays valid, which is
//! how metadata is removed from videos and similar containers. `into_plan`
//! turns the patches into a `Plan` (copy the untouched parts, write the
//! patched parts).

use super::model::Plan;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Patch {
    start: u64,
    end: u64,
    head: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Patches {
    items: Vec<Patch>,
}

impl Patches {
    pub fn new() -> Self {
        Patches::default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Replaces [start, end) by `head` followed by zeros.
    pub fn replace(&mut self, start: u64, end: u64, head: Vec<u8>) {
        self.items.push(Patch { start, end, head });
    }

    /// Overwrites [start, end) with zeros.
    pub fn zero(&mut self, start: u64, end: u64) {
        self.replace(start, end, Vec::new());
    }

    /// Builds the plan for a file of `len` bytes. Empty ranges are ignored.
    /// Overlapping patches, patches beyond the end of the file and a `head`
    /// longer than its range are errors.
    pub fn into_plan(self, len: u64) -> Result<Plan, String> {
        let mut items: Vec<Patch> = self
            .items
            .into_iter()
            .filter(|patch| patch.end > patch.start)
            .collect();
        items.sort_by_key(|patch| patch.start);
        let mut plan = Plan::new();
        let mut pos = 0u64;
        for patch in &items {
            let size = patch.end - patch.start;
            if patch.start < pos || patch.end > len || patch.head.len() as u64 > size {
                return Err("unusual file structure".to_string());
            }
            plan.copy(pos, patch.start - pos);
            plan.insert(patch.head.clone());
            plan.zeros(size - patch.head.len() as u64);
            pos = patch.end;
        }
        plan.copy(pos, len - pos);
        Ok(plan)
    }
}

/// The start of a "free" box (skipped by every player) that replaces a box
/// with the same header length and the same total size. None when the size
/// does not fit the header.
pub fn free_head(header_len: u64, total: u64) -> Option<Vec<u8>> {
    match header_len {
        8 => {
            let size = u32::try_from(total).ok()?;
            let mut out = size.to_be_bytes().to_vec();
            out.extend_from_slice(b"free");
            Some(out)
        }
        16 => {
            let mut out = vec![0, 0, 0, 1];
            out.extend_from_slice(b"free");
            out.extend_from_slice(&total.to_be_bytes());
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::metadata_remover::executor::apply_plan;
    use crate::tools::metadata_remover::model::Op;
    use std::io::Cursor;

    #[test]
    fn no_patches_means_an_identical_copy() {
        let patches = Patches::new();
        assert!(patches.is_empty());
        let plan = patches.into_plan(10).unwrap();
        assert!(plan.is_identity(10));
        let mut empty = Patches::new();
        empty.zero(3, 3);
        assert!(empty.into_plan(10).unwrap().is_identity(10));
    }

    #[test]
    fn patches_become_copy_insert_and_zero_steps_in_order() {
        let mut patches = Patches::new();
        patches.zero(8, 10);
        patches.replace(2, 6, vec![9, 9]);
        let plan = patches.into_plan(12).unwrap();
        assert_eq!(
            plan.ops,
            vec![
                Op::Copy { start: 0, len: 2 },
                Op::Insert(vec![9, 9]),
                Op::Zeros(2),
                Op::Copy { start: 6, len: 2 },
                Op::Zeros(2),
                Op::Copy { start: 10, len: 2 },
            ]
        );
        assert_eq!(plan.output_len(), 12);

        let mut out: Vec<u8> = Vec::new();
        apply_plan(
            &mut Cursor::new(b"ABCDEFGHIJKL".to_vec()),
            &plan,
            &mut out,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(out, b"AB\x09\x09\0\0GH\0\0KL".to_vec());
    }

    #[test]
    fn invalid_patches_are_rejected() {
        let mut overlap = Patches::new();
        overlap.replace(2, 6, vec![1]);
        overlap.zero(5, 8);
        assert!(overlap.into_plan(12).is_err());

        let mut beyond = Patches::new();
        beyond.zero(5, 50);
        assert!(beyond.into_plan(10).is_err());

        let mut too_long = Patches::new();
        too_long.replace(0, 2, vec![1, 2, 3]);
        assert!(too_long.into_plan(10).is_err());
    }

    #[test]
    fn patches_that_touch_each_other_are_allowed() {
        let mut patches = Patches::new();
        patches.zero(2, 4);
        patches.zero(4, 6);
        let plan = patches.into_plan(8).unwrap();
        assert_eq!(
            plan.ops,
            vec![
                Op::Copy { start: 0, len: 2 },
                Op::Zeros(2),
                Op::Zeros(2),
                Op::Copy { start: 6, len: 2 },
            ]
        );
    }

    #[test]
    fn free_box_headers_keep_the_size_and_the_header_length() {
        assert_eq!(
            free_head(8, 20),
            Some(vec![0, 0, 0, 20, b'f', b'r', b'e', b'e'])
        );
        assert_eq!(
            free_head(16, 40),
            Some(vec![
                0, 0, 0, 1, b'f', b'r', b'e', b'e', 0, 0, 0, 0, 0, 0, 0, 40
            ])
        );
        assert_eq!(free_head(8, u64::from(u32::MAX) + 1), None);
        assert_eq!(free_head(12, 20), None);
    }
}
