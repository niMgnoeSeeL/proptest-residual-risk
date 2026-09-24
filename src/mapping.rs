//! LLVM's coverage mapping, read from the running test binary: which source regions each
//! function has, and how each region's execution count follows from the function's counters.
//!
//! Why: LLVM gives no counter of its own to code whose count it can work out from other
//! counters. In `clamp` below, "inside" has no counter; its count is entry − below − above.
//!
//! ```text
//! fn clamp(x: i64, lo: i64, hi: i64) -> i64 {
//!     if x < lo { return lo; }   // "below": its own counter
//!     if x > hi { return hi; }   // "above": its own counter
//!     x                          // "inside": entry − below − above
//! }
//! ```
//!
//! A test case that leaves by "inside" raises only the entry counter, so counting counters misses
//! the first run of "inside". Counting regions does not: the mapping says how to compute every
//! region's count, and a region ran in a test case when that count went up.
//!
//! Formats (all little-endian), as written by LLVM 19-22 (rustc 1.86-1.98):
//! - `__llvm_covmap`: one record per compilation unit: four u32 (0, filenames size, 0, version),
//!   then the filenames blob, padded to 8 bytes. A function refers to its unit's blob by the low
//!   64 bits of the blob's MD5.
//! - `__llvm_covfun`: one record per function: u64 name hash, u32 data size, u64 function hash,
//!   u64 filenames hash, then the mapping data, padded to 8 bytes.
//! - The profile runtime's data records (64 bytes each): u64 name hash, u64 function hash,
//!   i64 offset from the record to the function's first counter, ..., u32 number of counters at
//!   byte 48 (the layout crabcheck reads, `crates/crabcheck/src/coverage/counters.rs`).

use std::collections::HashMap;

/// How a region's count is computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Term {
    Zero,
    /// The function's local counter number.
    Counter(u32),
    Sub(u32),
    Add(u32),
}

/// One source region of one function.
#[derive(Clone, Debug)]
pub struct Region {
    pub term: Term,
    pub file: u32,
    pub line_start: u32,
    pub col_start: u32,
    pub line_end: u32,
    pub col_end: u32,
}

/// One function's mapping.
#[derive(Clone, Debug)]
pub struct Function {
    pub name_ref: u64,
    pub func_hash: u64,
    /// Global file index (into `Mapping::files`) of each of the function's local file ids.
    pub files: Vec<u32>,
    /// Expressions: (left, right); the kind comes from the term that refers to the expression.
    pub exprs: Vec<(Term, Term)>,
    pub regions: Vec<Region>,
}

/// Everything the binary holds.
#[derive(Clone, Debug, Default)]
pub struct Mapping {
    pub files: Vec<String>,
    pub functions: Vec<Function>,
}

fn uleb(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let byte = *b.get(*pos)?;
        *pos += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn decode_term(v: u64) -> Term {
    let id = (v >> 2) as u32;
    match v & 3 {
        0 => Term::Zero,
        1 => Term::Counter(id),
        2 => Term::Sub(id),
        _ => Term::Add(id),
    }
}

/// The filenames of one compilation unit. From format version 6 on, the first name is the
/// compilation directory and relative names are relative to it.
fn filenames(blob: &[u8], version: u32) -> Option<Vec<String>> {
    let mut pos = 0;
    let n = uleb(blob, &mut pos)? as usize;
    let uncompressed = uleb(blob, &mut pos)? as usize;
    let compressed = uleb(blob, &mut pos)? as usize;
    let data: Vec<u8> = if compressed > 0 {
        let raw = blob.get(pos..pos + compressed)?;
        let out = miniz_oxide::inflate::decompress_to_vec_zlib(raw).ok()?;
        if out.len() != uncompressed {
            return None;
        }
        out
    } else {
        blob.get(pos..)?.to_vec()
    };
    let mut p = 0;
    let mut names = Vec::with_capacity(n);
    for _ in 0..n {
        let len = uleb(&data, &mut p)? as usize;
        names.push(String::from_utf8_lossy(data.get(p..p + len)?).into_owned());
        p += len;
    }
    // Format version numbers are stored minus one: version 6 of the format is stored as 5.
    if version >= 5 && !names.is_empty() {
        let dir = names[0].clone();
        for name in names.iter_mut().skip(1) {
            if !name.starts_with('/') && !dir.is_empty() {
                *name = format!("{dir}/{name}");
            }
        }
    }
    Some(names)
}

/// A function record's file ids, expressions and regions.
type Parsed = (Vec<u32>, Vec<(Term, Term)>, Vec<Region>);

fn parse_function(data: &[u8], unit_files: &[u32]) -> Option<Parsed> {
    let mut pos = 0;
    let nfiles = uleb(data, &mut pos)? as usize;
    let mut files = Vec::with_capacity(nfiles);
    for _ in 0..nfiles {
        let i = uleb(data, &mut pos)? as usize;
        files.push(*unit_files.get(i)?);
    }
    let nexpr = uleb(data, &mut pos)? as usize;
    let mut exprs = Vec::with_capacity(nexpr);
    for _ in 0..nexpr {
        let l = decode_term(uleb(data, &mut pos)?);
        let r = decode_term(uleb(data, &mut pos)?);
        exprs.push((l, r));
    }
    let mut regions = Vec::new();
    for &file in &files {
        let nregions = uleb(data, &mut pos)? as usize;
        let mut line = 0u32;
        for _ in 0..nregions {
            let head = uleb(data, &mut pos)?;
            // Code regions carry their term; the other kinds are recognised and skipped.
            let mut code = None;
            let mut branch: Option<(Term, Term)> = None;
            if head & 3 != 0 {
                code = Some(decode_term(head));
            } else if head & 4 != 0 {
                // expansion region: its code is in the expanded file's own regions
            } else {
                match head >> 3 {
                    0 => code = Some(Term::Zero),
                    2 => {} // skipped region
                    4 => {
                        let t = decode_term(uleb(data, &mut pos)?);
                        let f = decode_term(uleb(data, &mut pos)?);
                        branch = Some((t, f));
                    }
                    5 => {
                        // MC/DC decision: bitmap index, number of conditions
                        uleb(data, &mut pos)?;
                        uleb(data, &mut pos)?;
                    }
                    6 => {
                        // MC/DC branch: two terms, then condition id, true and false ids
                        uleb(data, &mut pos)?;
                        uleb(data, &mut pos)?;
                        uleb(data, &mut pos)?;
                        uleb(data, &mut pos)?;
                        uleb(data, &mut pos)?;
                    }
                    _ => return None,
                }
            }
            let delta = uleb(data, &mut pos)? as u32;
            let col_start = uleb(data, &mut pos)? as u32;
            let nlines = uleb(data, &mut pos)? as u32;
            let col_end_raw = uleb(data, &mut pos)? as u32;
            line += delta;
            let gap = col_end_raw & (1 << 31) != 0;
            let col_end = col_end_raw & !(1 << 31);
            let make = |term| Region {
                term,
                file,
                line_start: line,
                col_start,
                line_end: line + nlines,
                col_end,
            };
            if gap {
                continue;
            }
            if let Some(term) = code {
                regions.push(make(term));
            }
            if let Some((t, f)) = branch {
                regions.push(make(t));
                regions.push(make(f));
            }
        }
    }
    Some((files, exprs, regions))
}

/// Parse the two coverage sections of a binary.
pub fn parse(covmap: &[u8], covfun: &[u8]) -> Option<Mapping> {
    let mut mapping = Mapping::default();
    // filenames hash -> global indices of that unit's files
    let mut units: HashMap<u64, Vec<u32>> = HashMap::new();
    let mut pos = 0;
    while pos + 16 <= covmap.len() {
        let size = u32_at(covmap, pos + 4)? as usize;
        let version = u32_at(covmap, pos + 12)?;
        let blob = covmap.get(pos + 16..pos + 16 + size)?;
        let hash = u64::from_le_bytes(md5::compute(blob).0[..8].try_into().ok()?);
        let names = filenames(blob, version)?;
        let base = mapping.files.len() as u32;
        let ids = (0..names.len() as u32).map(|i| base + i).collect();
        mapping.files.extend(names);
        units.insert(hash, ids);
        pos = (pos + 16 + size + 7) & !7;
    }
    let mut pos = 0;
    while pos + 28 <= covfun.len() {
        let name_ref = u64_at(covfun, pos)?;
        let size = u32_at(covfun, pos + 8)? as usize;
        let func_hash = u64_at(covfun, pos + 12)?;
        let files_ref = u64_at(covfun, pos + 20)?;
        let data = covfun.get(pos + 28..pos + 28 + size)?;
        pos = (pos + 28 + size + 7) & !7;
        if name_ref == 0 && size == 0 {
            continue;
        }
        let Some(unit) = units.get(&files_ref) else {
            continue;
        };
        if let Some((files, exprs, regions)) = parse_function(data, unit) {
            mapping.functions.push(Function {
                name_ref,
                func_hash,
                files,
                exprs,
                regions,
            });
        }
    }
    Some(mapping)
}

/// The value of `term` given the function's counter increments `delta`.
pub fn eval(term: Term, exprs: &[(Term, Term)], delta: &dyn Fn(u32) -> i64, depth: u32) -> i64 {
    if depth > 256 {
        return 0;
    }
    match term {
        Term::Zero => 0,
        Term::Counter(i) => delta(i),
        Term::Sub(e) | Term::Add(e) => {
            let Some(&(l, r)) = exprs.get(e as usize) else {
                return 0;
            };
            let a = eval(l, exprs, delta, depth + 1);
            let b = eval(r, exprs, delta, depth + 1);
            if matches!(term, Term::Sub(_)) {
                a - b
            } else {
                a + b
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_decode() {
        assert_eq!(decode_term(0), Term::Zero);
        assert_eq!(decode_term((7 << 2) | 1), Term::Counter(7));
        assert_eq!(decode_term((3 << 2) | 2), Term::Sub(3));
        assert_eq!(decode_term((3 << 2) | 3), Term::Add(3));
    }

    #[test]
    fn inside_is_entry_minus_below_minus_above() {
        // counters: 0 entry, 1 below, 2 above; expr0 = c0 - c1, expr1 = expr0 - c2 ("inside")
        let exprs = vec![
            (Term::Counter(0), Term::Counter(1)),
            (Term::Sub(0), Term::Counter(2)),
        ];
        let inside = Term::Sub(1);
        let run = |d: [i64; 3]| eval(inside, &exprs, &|i| d[i as usize], 0);
        assert_eq!(run([1, 0, 0]), 1); // left by "inside"
        assert_eq!(run([1, 1, 0]), 0); // left by "below"
        assert_eq!(run([256, 94, 78]), 84);
    }
}
