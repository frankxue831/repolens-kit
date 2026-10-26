use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A node in the file-level dependency graph.
///
/// Corresponds to one entry in the `nodes` array of `graph.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    /// File path relative to the repo root.
    pub id: String,
    /// Symbol names extracted from this file (class names, functions, etc.).
    pub symbols: Vec<String>,
}

/// A directed edge between two nodes in the file graph.
///
/// Corresponds to one entry in the `edges` array of `graph.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source file path (relative to repo root).
    pub source: String,
    /// Target file path (relative to repo root).
    pub target: String,
    /// Relationship type: one of "imports", "type_import" (Python import under
    /// `if TYPE_CHECKING:`; not a runtime dependency), "extends",
    /// "implements", "calls", "type_ref", "declares".
    #[serde(rename = "type")]
    pub edge_type: String,
    /// Optional detail about the relationship (e.g., import symbol name, method called).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The complete file-level dependency graph stored in `graph.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Resolution quality metrics for the graph, stored in `graph-metrics.json`.
///
/// Uses camelCase field names as emitted by the Java serializer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphResolutionMetrics {
    pub total_relationships: i32,
    pub resolved_edges: i32,
    pub unresolved_edges: i32,
    /// Per-language breakdown: language name → [resolved, unresolved].
    /// Omitted entirely when None.
    ///
    /// A `BTreeMap` (not `HashMap`) so the keys serialize in a stable, sorted
    /// order — `graph-metrics.json` must be byte-identical run-to-run, and
    /// `HashMap` iteration order is randomized per process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_language: Option<BTreeMap<String, Vec<i32>>>,
    /// Relative imports (`./pkg/gm_web.js`, Python `.mod`) that matched no
    /// file in the scan, for example a JS import of a build output folder.
    /// Omitted when there are none, so packs without them are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_imports: Option<UnresolvedImports>,
}

/// Exact count plus a sorted list of unresolved imports.
///
/// In `graph-metrics.json` the list holds every import up to
/// [`Self::STORED_CAP`]; in an answer it holds the matching ones up to
/// [`Self::SAMPLE_CAP`]. `count` is always exact (#42: the match runs on
/// the stored list first, and only what is shown is capped).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedImports {
    pub count: usize,
    pub sample: Vec<UnresolvedImport>,
    /// How many of `count` are imports that weren't stored, so they could
    /// not be checked against the file asked about and count as possibly
    /// relevant. Omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unchecked: usize,
    /// In an answer: how many unresolved imports the pack counted in all,
    /// and how many it stored. Not serialized.
    #[serde(skip)]
    pub total: usize,
    #[serde(skip)]
    pub stored: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One import that matched no file: where it is and what it names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedImport {
    pub source: String,
    pub import: String,
}

impl UnresolvedImports {
    /// Most entries shown in an answer.
    pub const SAMPLE_CAP: usize = 50;
    /// Most entries stored in the pack. Beyond it the rest are counted,
    /// and an answer treats them as possibly relevant.
    pub const STORED_CAP: usize = 5000;

    /// Sorted, deduplicated, stored up to [`Self::STORED_CAP`]; `None`
    /// when `all` is empty.
    pub fn from_all(mut all: Vec<UnresolvedImport>) -> Option<Self> {
        all.sort();
        all.dedup();
        if all.is_empty() {
            return None;
        }
        let count = all.len();
        all.truncate(Self::STORED_CAP);
        let stored = all.len();
        Some(Self {
            count,
            sample: all,
            unchecked: 0,
            total: count,
            stored,
        })
    }

    /// The imports written in `file`, or `None` if it has none.
    pub fn only_from(self, file: &str) -> Option<Self> {
        self.only_where(|u| u.source == file)
    }

    /// The imports matching `keep`, or `None` if none do and every import
    /// was checked.
    ///
    /// Matches over the whole stored list, then caps what is kept at
    /// [`Self::SAMPLE_CAP`]. Imports counted but not stored (a capped
    /// pack, including packs written before #42 with a 50-entry list)
    /// can't be checked, so they are kept as `unchecked`.
    pub fn only_where(self, keep: impl Fn(&UnresolvedImport) -> bool) -> Option<Self> {
        // `count` covers stored and unstored imports alike.
        let unchecked = self
            .count
            .saturating_sub(self.sample.len())
            .max(self.unchecked);
        let (total, stored) = (self.count, self.sample.len());
        let mut sample: Vec<_> = self.sample.into_iter().filter(|u| keep(u)).collect();
        if sample.is_empty() && unchecked == 0 {
            return None;
        }
        let matched = sample.len();
        sample.truncate(Self::SAMPLE_CAP);
        Some(Self {
            count: matched + unchecked,
            sample,
            unchecked,
            total,
            stored,
        })
    }

    /// The pack stored fewer imports than it counted and fewer than
    /// [`Self::STORED_CAP`]: it was written before #42, when only 50 were
    /// kept, so a rescan stores them all.
    pub fn from_older_pack(&self) -> bool {
        self.unchecked > 0 && self.stored < Self::STORED_CAP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `graph-metrics.json` must be byte-identical run-to-run. The per-language
    /// breakdown previously serialized in `HashMap` iteration order, which is
    /// randomized per process and produced byte-different output on
    /// multi-language repos (the keys reordered; the values were identical).
    /// Serializing through an ordered map keeps the keys sorted and the file
    /// stable. Insert in deliberately non-sorted order and require the
    /// serialized keys to come out sorted.
    #[test]
    fn per_language_serializes_with_sorted_keys() {
        let langs = [
            "TypeScript",
            "Rust",
            "Swift",
            "Scala",
            "Ruby",
            "Python",
            "Kotlin",
            "JavaScript",
            "Java",
            "Go",
        ];
        let per_language = langs
            .iter()
            .enumerate()
            .map(|(i, l)| (l.to_string(), vec![i as i32, 0]))
            .collect();
        let metrics = GraphResolutionMetrics {
            total_relationships: 0,
            resolved_edges: 0,
            unresolved_edges: 0,
            per_language: Some(per_language),
            unresolved_imports: None,
        };

        let json = serde_json::to_string(&metrics).unwrap();

        let mut sorted_langs = langs;
        sorted_langs.sort_unstable();
        let positions: Vec<usize> = sorted_langs
            .iter()
            .map(|k| {
                json.find(&format!("\"{k}\":"))
                    .unwrap_or_else(|| panic!("key {k} missing from {json}"))
            })
            .collect();

        let mut ascending = positions.clone();
        ascending.sort_unstable();
        assert_eq!(
            positions, ascending,
            "perLanguage keys must serialize in sorted order, got: {json}"
        );
    }

    fn imp(source: &str, import: &str) -> UnresolvedImport {
        UnresolvedImport {
            source: source.into(),
            import: import.into(),
        }
    }

    /// #42: the stored list is not cut at the 50 shown, so a match past
    /// the 50th import is still found, and the count stays exact.
    #[test]
    fn match_runs_on_the_stored_list_before_the_cap() {
        let mut all: Vec<_> = (0..60)
            .map(|i| imp(&format!("a/b{i:02}.js"), &format!("./m{i:02}.js")))
            .collect();
        all.push(imp("www/app.js", "./pkg/gm_web.js"));
        let u = UnresolvedImports::from_all(all).unwrap();
        assert_eq!((u.count, u.sample.len(), u.unchecked), (61, 61, 0));
        let hit = u
            .clone()
            .only_where(|i| i.import == "./pkg/gm_web.js")
            .unwrap();
        assert_eq!((hit.count, hit.sample.len(), hit.unchecked), (1, 1, 0));
        let all = u.clone().only_where(|_| true).unwrap();
        assert_eq!(
            (all.count, all.sample.len()),
            (61, UnresolvedImports::SAMPLE_CAP)
        );
        assert!(u.only_where(|_| false).is_none());
    }

    /// A pack whose list was capped (packs before #42 kept 50): imports
    /// that weren't stored can't be ruled out, so they are kept as
    /// unchecked and the answer is never empty.
    #[test]
    fn imports_that_were_not_stored_count_as_unchecked() {
        let old = UnresolvedImports {
            count: 62,
            sample: (0..50)
                .map(|i| imp("a.js", &format!("./m{i:02}.js")))
                .collect(),
            unchecked: 0,
            total: 0,
            stored: 0,
        };
        let u = old.clone().only_where(|_| false).unwrap();
        assert_eq!((u.count, u.sample.len(), u.unchecked), (12, 0, 12));
        assert_eq!((u.total, u.stored), (62, 50));
        assert!(u.from_older_pack());
        let u = old.only_where(|i| i.import == "./m03.js").unwrap();
        assert_eq!((u.count, u.sample.len(), u.unchecked), (13, 1, 12));
        // Old packs read without the field.
        let json = r#"{"count":1,"sample":[{"source":"a.js","import":"./x.js"}]}"#;
        let u: UnresolvedImports = serde_json::from_str(json).unwrap();
        assert_eq!(u.unchecked, 0);
        assert!(!serde_json::to_string(&u).unwrap().contains("unchecked"));
    }
}
