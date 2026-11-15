//! Writes a `Plan` into any writer as a stream: copy ranges are read from
//! the source in chunks, so even a huge file needs only a small buffer.

use std::io::{self, Read, Seek, SeekFrom, Write};

use super::model::{Op, Plan};

const COPY_BUFFER_BYTES: usize = 256 * 1024;
const ZERO_CHUNK_BYTES: usize = 64 * 1024;

static ZERO_CHUNK: [u8; ZERO_CHUNK_BYTES] = [0u8; ZERO_CHUNK_BYTES];

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

fn internal() -> io::Error {
    io::Error::other("internal buffer error")
}

/// Writes the plan into `out` and returns the number of bytes written.
///
/// `progress(done, total)` is called after every written piece; returning
/// false cancels with an error of kind `Interrupted`. Reading past the end
/// of the source gives an `UnexpectedEof` error.
pub fn apply_plan<R: Read + Seek, W: Write>(
    source: &mut R,
    plan: &Plan,
    out: &mut W,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> io::Result<u64> {
    let total = plan.output_len();
    let mut done: u64 = 0;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    for op in &plan.ops {
        match op {
            Op::Copy { start, len } => {
                source.seek(SeekFrom::Start(*start))?;
                let mut remaining = *len;
                while remaining > 0 {
                    let take = remaining.min(COPY_BUFFER_BYTES as u64) as usize;
                    let chunk = buffer.get_mut(..take).ok_or_else(internal)?;
                    source.read_exact(chunk)?;
                    out.write_all(chunk)?;
                    remaining -= take as u64;
                    done = done.saturating_add(take as u64);
                    if !progress(done, total) {
                        return Err(cancelled());
                    }
                }
            }
            Op::Insert(bytes) => {
                out.write_all(bytes)?;
                done = done.saturating_add(bytes.len() as u64);
                if !progress(done, total) {
                    return Err(cancelled());
                }
            }
            Op::Zeros(count) => {
                let mut remaining = *count;
                while remaining > 0 {
                    let take = remaining.min(ZERO_CHUNK_BYTES as u64) as usize;
                    let chunk = ZERO_CHUNK.get(..take).ok_or_else(internal)?;
                    out.write_all(chunk)?;
                    remaining -= take as u64;
                    done = done.saturating_add(take as u64);
                    if !progress(done, total) {
                        return Err(cancelled());
                    }
                }
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SOURCE: &[u8] = b"0123456789ABCDEFGHIJ";

    fn sample_plan() -> Plan {
        let mut plan = Plan::new();
        plan.copy(0, 3);
        plan.insert(b"xy".to_vec());
        plan.zeros(2);
        plan.copy(10, 4);
        plan
    }

    #[test]
    fn copy_insert_and_zeros_are_written_in_order() {
        let mut out: Vec<u8> = Vec::new();
        let written = apply_plan(
            &mut Cursor::new(SOURCE.to_vec()),
            &sample_plan(),
            &mut out,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(out, b"012xy\0\0ABCD".to_vec());
        assert_eq!(written, 11);
    }

    #[test]
    fn progress_reports_every_step() {
        let mut calls: Vec<(u64, u64)> = Vec::new();
        let mut out: Vec<u8> = Vec::new();
        apply_plan(
            &mut Cursor::new(SOURCE.to_vec()),
            &sample_plan(),
            &mut out,
            &mut |done, total| {
                calls.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(calls, vec![(3, 11), (5, 11), (7, 11), (11, 11)]);
    }

    #[test]
    fn an_empty_plan_writes_nothing() {
        let mut out: Vec<u8> = Vec::new();
        let written = apply_plan(
            &mut Cursor::new(SOURCE.to_vec()),
            &Plan::new(),
            &mut out,
            &mut |_, _| panic!("no progress call expected"),
        )
        .unwrap();
        assert_eq!(written, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn cancel_stops_with_an_interrupted_error() {
        let mut plan = Plan::new();
        plan.copy(0, 10);
        let mut out: Vec<u8> = Vec::new();
        let error = apply_plan(
            &mut Cursor::new(SOURCE.to_vec()),
            &plan,
            &mut out,
            &mut |_, _| false,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn reading_past_the_end_fails() {
        let mut plan = Plan::new();
        plan.copy(15, 10);
        let mut out: Vec<u8> = Vec::new();
        let error = apply_plan(
            &mut Cursor::new(SOURCE.to_vec()),
            &plan,
            &mut out,
            &mut |_, _| true,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn large_copies_are_chunked() {
        let source: Vec<u8> = (0..600_000usize).map(|i| (i % 251) as u8).collect();
        let mut plan = Plan::new();
        plan.copy(100, 550_000);
        let mut calls: Vec<(u64, u64)> = Vec::new();
        let mut out: Vec<u8> = Vec::new();
        apply_plan(
            &mut Cursor::new(source.clone()),
            &plan,
            &mut out,
            &mut |done, total| {
                calls.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(out, source[100..550_100].to_vec());
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0], (262_144, 550_000));
        assert_eq!(calls[2], (550_000, 550_000));
    }

    #[test]
    fn large_zero_runs_are_chunked() {
        let mut plan = Plan::new();
        plan.zeros(100_000);
        let mut calls: Vec<(u64, u64)> = Vec::new();
        let mut out: Vec<u8> = Vec::new();
        apply_plan(
            &mut Cursor::new(Vec::new()),
            &plan,
            &mut out,
            &mut |done, total| {
                calls.push((done, total));
                true
            },
        )
        .unwrap();
        assert_eq!(out.len(), 100_000);
        assert!(out.iter().all(|byte| *byte == 0));
        assert_eq!(calls, vec![(65_536, 100_000), (100_000, 100_000)]);
    }
}
