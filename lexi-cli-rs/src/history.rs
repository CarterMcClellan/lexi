//! Command history (`~/.lexi-cli/history`): one command per line, oldest first
//! — the same plain format the Python CLI reads and writes.

use std::io::Write;
use std::path::PathBuf;

fn path() -> PathBuf {
    lexi_host::config::lexi_home().join("history")
}

pub(crate) fn load() -> Vec<String> {
    std::fs::read_to_string(path())
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Append one entry (newlines flattened so the file stays one-per-line).
pub(crate) fn append(entry: &str) -> std::io::Result<()> {
    let entry = entry.trim().replace(['\n', '\r'], " ");
    if entry.is_empty() {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path())?;
    writeln!(f, "{entry}")
}

pub(crate) fn clear() -> std::io::Result<()> {
    std::fs::write(path(), "")
}

/// Resolve a `#N` reference: positive N counts from the start (1-based),
/// negative from the end (`#-1` is the last entry).
pub(crate) fn entry(history: &[String], index: i64) -> Result<String, String> {
    if index == 0 {
        return Err("history index 0 is invalid; use positive or negative numbers".into());
    }
    let len = history.len() as i64;
    let resolved = if index > 0 { index } else { len + index + 1 };
    if resolved <= 0 || resolved > len {
        return Err(format!("history index {index} is out of range"));
    }
    let target = history[(resolved - 1) as usize].clone();
    if target.starts_with('#') {
        return Err("cannot re-run a history reference (#...) from history".into());
    }
    Ok(target)
}

/// The `N: command` listing: all entries, the last `count` (count > 0), or the
/// first `|count|` (count < 0).
pub(crate) fn listing(history: &[String], count: Option<i64>) -> Vec<(usize, &str)> {
    let n = history.len();
    let (start, end) = match count {
        None | Some(0) => (0, n),
        Some(c) if c > 0 => (n.saturating_sub(c as usize), n),
        Some(c) => (0, (c.unsigned_abs() as usize).min(n)),
    };
    (start..end).map(|i| (i + 1, history[i].as_str())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Vec<String> {
        ["a", "b", "#1", "c"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn resolves_references() {
        assert_eq!(entry(&h(), 1).unwrap(), "a");
        assert_eq!(entry(&h(), -1).unwrap(), "c");
        assert!(entry(&h(), 0).is_err());
        assert!(entry(&h(), 9).is_err());
        assert!(entry(&h(), 3).unwrap_err().contains("history reference"));
    }

    #[test]
    fn lists_windows() {
        assert_eq!(listing(&h(), None).len(), 4);
        assert_eq!(listing(&h(), Some(2)), vec![(3, "#1"), (4, "c")]);
        assert_eq!(listing(&h(), Some(-1)), vec![(1, "a")]);
    }
}
