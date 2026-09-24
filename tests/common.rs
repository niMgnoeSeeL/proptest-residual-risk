// Reads the JSON line the crate wrote for a test.
use std::path::PathBuf;

pub fn out_dir() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("proptest-residual-risk")
}

pub fn last_line(test: &str) -> String {
    let name: String = test
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let text = std::fs::read_to_string(out_dir().join(format!("{name}.jsonl"))).unwrap();
    text.lines().last().unwrap().to_string()
}

pub fn num(line: &str, key: &str) -> f64 {
    let pat = format!("\"{key}\":");
    let rest = &line[line.find(&pat).unwrap() + pat.len()..];
    let end = rest.find([',', '}']).unwrap();
    rest[..end].parse().unwrap()
}

#[allow(dead_code)]
pub fn flag(line: &str, key: &str) -> bool {
    line.contains(&format!("\"{key}\":true"))
}
