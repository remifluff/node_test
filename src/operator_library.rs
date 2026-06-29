//! Operator catalog loaded from `operators.txt` (fragment_interlay format).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorEntry {
    pub category: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct OperatorLibrary {
    entries: Vec<OperatorEntry>,
    source: Option<PathBuf>,
}

impl OperatorLibrary {
    /// Built-in catalog (see repo-root `operators.txt`).
    pub fn embedded() -> Self {
        Self::from_str(include_str!("../operators.txt")).with_source(None)
    }

    /// Load from disk, falling back to [`Self::embedded`] if missing or invalid.
    pub fn load_preferred() -> Self {
        for path in preferred_operator_paths() {
            if path.is_file() {
                match Self::load(&path) {
                    Ok(lib) => return lib,
                    Err(err) => {
                        log::warn!("operator library {path:?}: {err}");
                    }
                }
            }
        }
        Self::embedded()
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Ok(Self::from_str(&text).with_source(Some(path.to_path_buf())))
    }

    pub fn from_str(text: &str) -> Self {
        Self {
            entries: parse_operator_list(text),
            source: None,
        }
    }

    fn with_source(mut self, source: Option<PathBuf>) -> Self {
        self.source = source;
        self
    }

    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[OperatorEntry] {
        &self.entries
    }

    /// Prefix matches for autocomplete, case-insensitive, preserving catalog order.
    pub fn match_prefix<'a>(&'a self, prefix: &str, limit: usize) -> Vec<&'a OperatorEntry> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let needle = prefix.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.name.to_lowercase().starts_with(&needle))
            .take(limit)
            .collect()
    }
}

fn preferred_operator_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(env) = std::env::var("OPERATORS_FILE") {
        paths.push(PathBuf::from(env));
    }
    paths.push(PathBuf::from("operators.txt"));
    paths.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("operators.txt"));
    paths.push(
        PathBuf::from("/Users/remi/Documents/Max 9/Packages/fragment_interlay/operator_list.txt"),
    );
    paths
}

fn parse_operator_list(text: &str) -> Vec<OperatorEntry> {
    let mut category = String::from("general");
    let mut out = Vec::new();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.eq_ignore_ascii_case("operator list") {
            continue;
        }

        if let Some((name, description)) = split_name_description(line) {
            out.push(OperatorEntry {
                category: category.clone(),
                name,
                description,
            });
        } else {
            category = line.to_string();
        }
    }

    out
}

/// Split `op - description` or `op- description` (genjit list typos).
fn split_name_description(line: &str) -> Option<(String, String)> {
    if let Some((name, desc)) = line.split_once(" - ") {
        let name = name.trim();
        let desc = desc.trim();
        if !name.is_empty() && !desc.is_empty() {
            return Some((name.to_string(), desc.to_string()));
        }
    }
    if let Some(pos) = line.find("- ") {
        if pos > 0 {
            let name = line[..pos].trim();
            let desc = line[pos + 2..].trim();
            if !name.is_empty() && !desc.is_empty() {
                return Some((name.to_string(), desc.to_string()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_subtract_and_min() {
        assert_eq!(
            split_name_description("- - Subtract inputs"),
            Some(("-".into(), "Subtract inputs".into()))
        );
        assert_eq!(
            split_name_description("min- The minimum of the inputs"),
            Some(("min".into(), "The minimum of the inputs".into()))
        );
    }

    #[test]
    fn embedded_non_empty_includes_pd_and_gen() {
        let lib = OperatorLibrary::embedded();
        assert!(lib.len() > 100);
        assert!(lib.entries().iter().any(|e| e.name == "osc~"));
        assert!(lib.entries().iter().any(|e| e.name == "swiz"));
    }

    #[test]
    fn match_prefix_case_insensitive() {
        let lib = OperatorLibrary::embedded();
        let hits = lib.match_prefix("Osc", 8);
        assert!(hits.iter().any(|e| e.name == "osc~"));
    }
}
