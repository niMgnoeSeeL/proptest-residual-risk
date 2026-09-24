//! What one test case ran: the LLVM coverage counters it raised and the source regions it ran,
//! both limited to the code under test.
//!
//! With `-C instrument-coverage`, LLVM keeps one array of counters for the whole process. We copy
//! it before a test case and compare after. The counters are process-wide, so test cases of
//! different tests running at the same time would mix: every observed test case holds one lock,
//! and tests that do not use this crate should not run in parallel (`--test-threads=1`, or
//! `cargo nextest`, which runs each test in its own process).
//!
//! Regions come from the coverage mapping in the test binary (`mapping.rs`): a region ran when
//! its count, computed from the function's counter increments, went up. Unlike counters, every
//! branch is a region, including a branch LLVM counts as "entry − the other branches".
//!
//! The code under test is every source file except dependencies (`.cargo/registry`,
//! `.cargo/git`), the standard library (`/rustc/…`), files under a `tests`, `benches` or
//! `examples` directory, and this crate. `RESIDUAL_RISK_CODE` (comma-separated path prefixes;
//! a prefix starting with `!` excludes, e.g. `src/,!src/bin/`) replaces that rule. Counters count only when they belong to a function with a region in the
//! code under test. Without a readable mapping, every counter counts and there are no regions.

#[cfg(feature = "coverage")]
mod imp {
    use crate::mapping::{self, Mapping, Term};
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    extern "C" {
        fn __llvm_profile_begin_counters() -> *mut u64;
        fn __llvm_profile_end_counters() -> *mut u64;
        fn __llvm_profile_begin_data() -> *const u8;
        fn __llvm_profile_end_data() -> *const u8;
    }

    /// The counter array, read as atomics: the instrumented code keeps writing to it, so a plain
    /// shared slice would let the optimiser reuse an earlier read (in a release build, the
    /// "after" copy came out equal to the "before" copy).
    fn counters() -> &'static [AtomicU64] {
        // SAFETY: the profiling runtime linked by -C instrument-coverage owns this array for the
        // life of the process; we only read it, with atomic loads.
        unsafe {
            let b = __llvm_profile_begin_counters();
            let e = __llvm_profile_end_counters();
            std::slice::from_raw_parts(b as *const AtomicU64, e.offset_from(b) as usize)
        }
    }

    /// Copies only the counters of the code under test (`Index::window`), not the whole array:
    /// the copy is the main cost per test case, and most counters belong to dependencies and to
    /// this crate itself.
    fn snapshot(window: &[(usize, usize)]) -> Vec<u64> {
        let all = counters();
        let mut out = Vec::with_capacity(window.iter().map(|w| w.1).sum());
        for &(start, len) in window {
            let end = (start + len).min(all.len());
            out.extend(
                all[start.min(end)..end]
                    .iter()
                    .map(|c| c.load(Ordering::Relaxed)),
            );
        }
        out
    }

    /// Size of one profile data record (LLVM 19-22).
    const RECORD: usize = 64;

    /// Each function's (name, hash) and the (start, length) of its counters in the counter array.
    type Records = HashMap<(u64, u64), (usize, usize)>;

    /// (name hash, function hash) -> (first counter index, number of counters), or why the
    /// records could not be read. The records of a readable layout cover the counter array
    /// exactly; any other total means the layout is not the one assumed here (LLVM 19-22).
    fn data_records() -> Result<Records, String> {
        let mut out = HashMap::new();
        let mut covered = 0usize;
        // SAFETY: the data section is static and laid out as in compiler-rt's InstrProfData.inc.
        unsafe {
            let begin = __llvm_profile_begin_data();
            let end = __llvm_profile_end_data();
            let len = end.offset_from(begin).max(0) as usize;
            let bytes = std::slice::from_raw_parts(begin, len);
            let base = __llvm_profile_begin_counters() as usize;
            for (i, r) in bytes.chunks_exact(RECORD).enumerate() {
                let name = u64::from_le_bytes(r[0..8].try_into().unwrap());
                let hash = u64::from_le_bytes(r[8..16].try_into().unwrap());
                let rel = i64::from_le_bytes(r[16..24].try_into().unwrap());
                let n = u32::from_le_bytes(r[48..52].try_into().unwrap()) as usize;
                let at = (begin as usize + i * RECORD) as i64 + rel;
                if n == 0 || (at as usize) < base {
                    continue;
                }
                out.insert((name, hash), ((at as usize - base) / 8, n));
                covered += n;
            }
            if len % RECORD != 0 || covered != counters().len() {
                return Err(format!(
                    "the LLVM profile data layout is not the one this crate reads (LLVM 19-22): \
                     records cover {covered} of {} counters",
                    counters().len()
                ));
            }
        }
        Ok(out)
    }

    fn read_mapping() -> Option<Mapping> {
        use object::{Object, ObjectSection};
        let bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
        let file = object::File::parse(&*bytes).ok()?;
        let covmap = file
            .section_by_name("__llvm_covmap")?
            .uncompressed_data()
            .ok()?;
        let covfun = file
            .section_by_name("__llvm_covfun")?
            .uncompressed_data()
            .ok()?;
        mapping::parse(&covmap, &covfun)
    }

    fn in_code_under_test(path: &str, prefixes: &Option<Vec<String>>) -> bool {
        if let Some(ps) = prefixes {
            let include = ps.iter().filter(|p| !p.starts_with('!'));
            let exclude = ps.iter().filter_map(|p| p.strip_prefix('!'));
            return include.clone().any(|p| path.starts_with(p.as_str()))
                && !exclude.into_iter().any(|p| path.starts_with(p));
        }
        let own = env!("CARGO_MANIFEST_DIR");
        let skip_dir = path
            .split('/')
            .any(|c| c == "tests" || c == "benches" || c == "examples");
        !(path.contains("/.cargo/registry/")
            || path.contains("/.cargo/git/")
            || path.starts_with("/rustc/")
            || path.starts_with(own)
            || skip_dir)
    }

    /// One function of the code under test, as needed per test case.
    struct Func {
        counters: std::ops::Range<usize>,
        /// Where this function's counters start in a snapshot.
        pos: usize,
        exprs: Vec<(Term, Term)>,
        /// (region id, term)
        regions: Vec<(u32, Term)>,
    }

    pub struct Index {
        funcs: Vec<Func>,
        /// (first counter, number of counters) copied per test case, in snapshot order.
        window: Vec<(usize, usize)>,
        counter_ok: Option<HashSet<u32>>,
        pub total_regions: u32,
        pub files: Vec<String>,
        /// Why counters could not be limited to the code under test and regions are missing.
        pub error: Option<String>,
    }

    fn build() -> Index {
        let prefixes = std::env::var("RESIDUAL_RISK_CODE").ok().map(|v| {
            let cwd = std::env::current_dir().unwrap_or_default();
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let (bang, s) = match s.strip_prefix('!') {
                        Some(rest) => ("!", rest),
                        None => ("", s),
                    };
                    let p = std::path::Path::new(s);
                    let abs = if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        cwd.join(p)
                    };
                    format!("{bang}{}", abs.to_string_lossy())
                })
                .collect::<Vec<_>>()
        });
        let empty = |why: String| Index {
            funcs: Vec::new(),
            window: vec![(0, counters().len())],
            counter_ok: None,
            total_regions: 0,
            files: Vec::new(),
            error: Some(why),
        };
        let Some(map) = read_mapping() else {
            return empty("the coverage mapping could not be read from the test binary".into());
        };
        let records = match data_records() {
            Ok(r) => r,
            Err(why) => return empty(why),
        };
        let keep: Vec<bool> = map
            .files
            .iter()
            .map(|f| in_code_under_test(f, &prefixes))
            .collect();
        let mut funcs = Vec::new();
        let mut counter_ok = HashSet::new();
        let mut next = 0u32;
        let mut files = std::collections::BTreeSet::new();
        // A function can be listed once per compilation unit that uses it; count it once.
        let mut seen = HashSet::new();
        for f in &map.functions {
            if !seen.insert((f.name_ref, f.func_hash)) {
                continue;
            }
            let mut regions = Vec::new();
            for r in &f.regions {
                if keep[r.file as usize] {
                    regions.push((next, r.term));
                    next += 1;
                    files.insert(map.files[r.file as usize].clone());
                }
            }
            if regions.is_empty() {
                continue;
            }
            let range = match records.get(&(f.name_ref, f.func_hash)) {
                Some(&(start, n)) => {
                    counter_ok.extend((start..start + n).map(|i| i as u32));
                    start..start + n
                }
                // Never run (no counters were allocated): its regions stay at zero.
                None => 0..0,
            };
            funcs.push(Func {
                pos: 0,
                counters: range,
                exprs: f.exprs.clone(),
                regions,
            });
        }
        let error = if funcs.iter().all(|f| f.counters.is_empty()) {
            Some(
                "no function of the code under test has counters (check RESIDUAL_RISK_CODE)".into(),
            )
        } else {
            None
        };
        // Copy each function's counters once, in address order, and remember where they land.
        funcs.sort_by_key(|f| f.counters.start);
        let mut window = Vec::new();
        let mut pos = 0;
        for f in funcs.iter_mut() {
            f.pos = pos;
            if !f.counters.is_empty() {
                window.push((f.counters.start, f.counters.len()));
                pos += f.counters.len();
            }
        }
        Index {
            funcs,
            window,
            counter_ok: Some(counter_ok),
            total_regions: next,
            files: files.into_iter().collect(),
            error,
        }
    }

    static INDEX_SECONDS: OnceLock<f64> = OnceLock::new();

    pub fn index() -> &'static Index {
        static INDEX: OnceLock<Index> = OnceLock::new();
        INDEX.get_or_init(|| {
            let start = std::time::Instant::now();
            let index = build();
            let _ = INDEX_SECONDS.set(start.elapsed().as_secs_f64());
            index
        })
    }

    /// How long reading the coverage mapping and indexing the code under test took; once per
    /// process, in the first test case that takes coverage.
    pub fn index_seconds() -> f64 {
        INDEX_SECONDS.get().copied().unwrap_or(0.0)
    }

    static LOCK: Mutex<()> = Mutex::new(());

    pub struct Snapshot {
        before: Vec<u64>,
        _guard: MutexGuard<'static, ()>,
    }

    pub fn begin() -> Snapshot {
        index();
        let guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        Snapshot {
            before: snapshot(&index().window),
            _guard: guard,
        }
    }

    /// The counters raised and the regions run between `begin` and now.
    pub fn end(s: Snapshot) -> (Vec<u32>, Vec<u32>) {
        let idx = index();
        let after = snapshot(&idx.window);
        let before = &s.before;
        let mut raised = Vec::new();
        let mut at = 0;
        for &(start, len) in &idx.window {
            for j in 0..len {
                if at + j < after.len() && after[at + j] != before[at + j] {
                    raised.push((start + j) as u32);
                }
            }
            at += len;
        }
        let mut regions = Vec::new();
        for f in &idx.funcs {
            let (p, len) = (f.pos, f.counters.len());
            if len == 0 || p + len > after.len() || !(p..p + len).any(|i| after[i] != before[i]) {
                continue;
            }
            let delta = |c: u32| -> i64 {
                let i = p + c as usize;
                if (c as usize) < len {
                    after[i].wrapping_sub(before[i]) as i64
                } else {
                    0
                }
            };
            for &(id, term) in &f.regions {
                if mapping::eval(term, &f.exprs, &delta, 0) > 0 {
                    regions.push(id);
                }
            }
        }
        (raised, regions)
    }

    pub fn total_regions() -> Option<u32> {
        let i = index();
        i.counter_ok.as_ref().map(|_| i.total_regions)
    }

    pub fn files() -> Vec<String> {
        index().files.clone()
    }

    pub fn error() -> Option<String> {
        index().error.clone()
    }

    /// How many counters are copied per test case.
    pub fn window_size() -> usize {
        index().window.iter().map(|w| w.1).sum()
    }

    pub const ON: bool = true;
}

#[cfg(not(feature = "coverage"))]
mod imp {
    pub struct Snapshot;
    pub fn begin() -> Snapshot {
        Snapshot
    }
    pub fn end(_: Snapshot) -> (Vec<u32>, Vec<u32>) {
        (Vec::new(), Vec::new())
    }
    pub fn total_regions() -> Option<u32> {
        None
    }
    pub fn files() -> Vec<String> {
        Vec::new()
    }
    pub fn error() -> Option<String> {
        None
    }
    pub fn window_size() -> usize {
        0
    }
    pub fn index_seconds() -> f64 {
        0.0
    }
    pub const ON: bool = false;
}

pub use imp::{begin, end, error, files, index_seconds, total_regions, window_size, Snapshot, ON};
