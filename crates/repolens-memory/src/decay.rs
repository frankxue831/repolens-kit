//! Time/relevance-based importance decay for the memory graph.
//!
//! [`MemoryDecayEngine::decay`] applies a half-life formula (30-day half-life)
//! to every node's importance score, applies a supersession multiplier for
//! nodes that have been superseded by another, clamps to a minimum floor
//! derived from access count, and prunes nodes that fall below 0.05.

use std::collections::HashSet;

use time::OffsetDateTime;
use tracing::{debug, info};

use repolens_core::model::memory::{GroundingStatus, MemoryGraph, MemoryRelation};

/// Importance half-life in days.
const HALF_LIFE_DAYS: f64 = 30.0;

/// Stateless engine that applies importance decay to a [`MemoryGraph`].
pub struct MemoryDecayEngine;

impl MemoryDecayEngine {
    /// Apply time/relevance decay to all nodes in `graph` and return a new
    /// [`MemoryGraph`] with updated importances.
    ///
    /// # Algorithm (per node)
    ///
    /// ```text
    /// since                   = max(node.last_accessed,
    ///                               node.last_decayed_at ?? graph.last_decay_run)
    /// days_since_access       = max(0, (now - since) in fractional days)
    /// access_factor           = 0.5 ^ (days_since_access / 30.0)    // 30-day half-life
    /// access_floor            = min(0.3, access_count * 0.02)
    /// superseded_multiplier   = if a Supersedes edge targets this node { 0.5 } else { 1.0 }
    /// contradicted_multiplier = if a Contradicts edge targets this node { 0.25 } else { 1.0 }
    /// grounding_floor         = if last_grounding == Confirmed { 0.4 } else { 0.0 }
    /// grounding_multiplier    = if last_grounding == Contradicted { 0.5 } else { 1.0 }
    /// floor                   = max(access_floor, grounding_floor)
    /// new_importance          = max(floor,
    ///                               importance * access_factor
    ///                                          * superseded_multiplier
    ///                                          * contradicted_multiplier
    ///                                          * grounding_multiplier)
    /// ```
    ///
    /// `Supersedes` and `Contradicts` multipliers compose when both edges
    /// target the same node (`0.5 * 0.25 = 0.125`). A node that is both
    /// replaced and asserted wrong decays eight times faster than a node
    /// that is neither.
    ///
    /// # Grounding coupling
    ///
    /// `node.last_grounding` is a persisted snapshot of the recall-time
    /// [`GroundingStatus`] (written by `repolens_app::grounding::record_grounding`).
    /// A `Confirmed` snapshot raises the floor to `0.4` (slow decay — the
    /// memory's anchors still resolve against the codebase), sitting just above
    /// the access-floor cap of `0.3`. A `Contradicted` snapshot applies a `0.5`
    /// multiplier (faster decay — the anchors have drifted), composing with the
    /// edge multipliers and still respecting the floor (no auto-delete).
    /// `Unverifiable` / `Unchecked` / `None` are neutral. The grounding
    /// multiplier is deliberately gentler than the memory↔memory `Contradicts`
    /// edge (`0.25`): pack-grounding contradiction is suggestive, not an
    /// authoritative memory-vs-memory assertion.
    ///
    /// Nodes with `new_importance < 0.05` are pruned; edges where either
    /// endpoint was pruned are also removed.
    ///
    /// The returned graph has `last_decay_run = Some(now)` and
    /// Returns `true` when lazy decay should run: either the graph has never
    /// been decayed, or more than one hour has elapsed since the last run.
    ///
    /// Callers (e.g. `recall_longterm`, `SummaryService::summarize`) use this
    /// to amortize decay work across reads without doing it on every call.
    pub fn should_run(graph: &MemoryGraph, now: OffsetDateTime) -> bool {
        match graph.last_decay_run {
            None => true,
            Some(last) => (now - last).whole_hours() >= 1,
        }
    }

    /// `updated_at = now`.
    pub fn decay(graph: &MemoryGraph, now: OffsetDateTime) -> MemoryGraph {
        info!(
            nodes = graph.nodes.len(),
            edges = graph.edges.len(),
            "Starting memory decay run"
        );

        let mut new_nodes = Vec::with_capacity(graph.nodes.len());
        let mut pruned_ids: HashSet<String> = HashSet::new();

        for node in &graph.nodes {
            // Elapsed time since this node's importance was last brought up to
            // date: the later of its last access and its last decay. Legacy
            // nodes without `last_decayed_at` fall back to the graph-level
            // `last_decay_run` (the time every node was last decayed), so a
            // migrated graph is never decayed twice for the same interval.
            let decayed_through = node.last_decayed_at.or(graph.last_decay_run);
            let since = match decayed_through {
                Some(t) if t > node.last_accessed => t,
                _ => node.last_accessed,
            };
            let days_since_access = ((now - since).as_seconds_f64() / 86_400.0).max(0.0);

            let access_factor = 0.5_f64.powf(days_since_access / HALF_LIFE_DAYS);

            let access_floor = f32::min(0.3, node.access_count as f32 * 0.02);

            // Persisted recall-time grounding snapshot couples into decay:
            // Confirmed raises a floor, Contradicted applies a multiplier.
            let grounding_floor = match &node.last_grounding {
                Some(GroundingStatus::Confirmed) => 0.4,
                _ => 0.0,
            };
            let grounding_multiplier = match &node.last_grounding {
                Some(GroundingStatus::Contradicted) => 0.5,
                _ => 1.0,
            };
            let floor = f32::max(access_floor, grounding_floor);

            let superseded_multiplier = if is_superseded(&node.id, &graph.edges) {
                0.5
            } else {
                1.0
            };

            let contradicted_multiplier = if is_contradicted(&node.id, &graph.edges) {
                0.25
            } else {
                1.0
            };

            let new_importance = f32::max(
                floor,
                node.importance
                    * access_factor as f32
                    * superseded_multiplier
                    * contradicted_multiplier
                    * grounding_multiplier,
            );
            // The same decay without the grounding penalty, carried
            // forward run to run, so recall can show the penalty as its
            // own score term (#14).
            let before_grounding = f32::max(
                floor,
                node.importance_before_grounding.unwrap_or(node.importance)
                    * access_factor as f32
                    * superseded_multiplier
                    * contradicted_multiplier,
            );

            if new_importance < 0.05 {
                debug!(
                    id = %node.id,
                    old_importance = node.importance,
                    new_importance,
                    "Pruning memory node below threshold"
                );
                pruned_ids.insert(node.id.clone());
            } else {
                debug!(
                    id = %node.id,
                    old_importance = node.importance,
                    new_importance,
                    days_since_access,
                    "Decayed memory node"
                );
                let mut updated = node.clone();
                updated.importance = new_importance;
                updated.importance_before_grounding =
                    (before_grounding - new_importance > f32::EPSILON).then_some(before_grounding);
                updated.last_decayed_at = Some(now);
                new_nodes.push(updated);
            }
        }

        // Remove edges where either endpoint was pruned.
        let new_edges = graph
            .edges
            .iter()
            .filter(|e| !pruned_ids.contains(&e.source_id) && !pruned_ids.contains(&e.target_id))
            .cloned()
            .collect::<Vec<_>>();

        info!(
            nodes_before = graph.nodes.len(),
            nodes_after = new_nodes.len(),
            pruned = pruned_ids.len(),
            edges_before = graph.edges.len(),
            edges_after = new_edges.len(),
            "Memory decay run complete"
        );

        MemoryGraph {
            schema_version: graph.schema_version.clone(),
            repo_root: graph.repo_root.clone(),
            nodes: new_nodes,
            edges: new_edges,
            last_decay_run: Some(now),
            updated_at: now,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Returns `true` if any edge in `edges` has `relation == Supersedes` and
/// `target_id == id` (i.e. the node has been superseded by something else).
fn is_superseded(id: &str, edges: &[repolens_core::model::memory::MemoryEdge]) -> bool {
    edges
        .iter()
        .any(|e| e.relation == MemoryRelation::Supersedes && e.target_id == id)
}

/// Returns `true` if any edge in `edges` has `relation == Contradicts` and
/// `target_id == id` (i.e. the node has been explicitly contradicted by
/// another memory). Mirrors [`is_superseded`] but triggers a stronger decay
/// multiplier — a contradicted memory is asserted wrong, not merely
/// replaced.
fn is_contradicted(id: &str, edges: &[repolens_core::model::memory::MemoryEdge]) -> bool {
    edges
        .iter()
        .any(|e| e.relation == MemoryRelation::Contradicts && e.target_id == id)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use repolens_core::model::memory::{MemoryEdge, MemoryRelation, MemorySource, MemoryType};
    use time::Duration;

    /// Convenience constructor for test nodes.
    fn make_node_at(
        id: &str,
        importance: f32,
        last_accessed: OffsetDateTime,
        access_count: i32,
    ) -> repolens_core::model::memory::MemoryNode {
        repolens_core::model::memory::MemoryNode {
            id: id.to_string(),
            content: format!("content for {id}"),
            memory_type: MemoryType::Pattern,
            importance,
            created: last_accessed,
            last_accessed,
            access_count,
            anchors: vec![],
            tags: vec![],
            source: MemorySource::Explicit,
            provenance: "test".to_string(),
            last_grounding: None,
            last_grounded_at: None,
            last_decayed_at: None,
            importance_before_grounding: None,
            anchor_sha256: Default::default(),
        }
    }

    fn empty_graph() -> MemoryGraph {
        let now = OffsetDateTime::now_utc();
        MemoryGraph {
            schema_version: "1.0".to_string(),
            repo_root: "/tmp/test".to_string(),
            nodes: vec![],
            edges: vec![],
            last_decay_run: None,
            updated_at: now,
        }
    }

    #[test]
    fn fresh_node_stays_unchanged() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        graph.nodes.push(make_node_at("n1", 0.8, now, 0));

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1);
        // access_factor = 0.5^(0/30) = 1.0, no supersession, floor = 0.0
        // new_importance = 0.8 * 1.0 * 1.0 = 0.8
        let delta = (result.nodes[0].importance - 0.8_f32).abs();
        assert!(
            delta < 1e-4,
            "fresh node importance should be ~0.8, got {}",
            result.nodes[0].importance
        );
    }

    #[test]
    fn old_node_decays() {
        let now = OffsetDateTime::now_utc();
        let old = now - Duration::days(60);
        let mut graph = empty_graph();
        graph.nodes.push(make_node_at("n1", 0.8, old, 0));

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1);
        // access_factor = 0.5^(60/30) = 0.25, floor = 0.0
        // new_importance = 0.8 * 0.25 = 0.2
        let importance = result.nodes[0].importance;
        assert!(
            importance < 0.8,
            "importance should decrease for 60-day-old node, got {importance}"
        );
        assert!(
            (importance - 0.2_f32).abs() < 1e-4,
            "expected ~0.2, got {importance}"
        );
    }

    #[test]
    fn access_floor_prevents_excessive_decay() {
        let now = OffsetDateTime::now_utc();
        let very_old = now - Duration::days(365);
        let mut graph = empty_graph();
        // access_count=20 → floor = min(0.3, 20*0.02) = min(0.3, 0.4) = 0.3
        graph.nodes.push(make_node_at("n1", 0.9, very_old, 20));

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1);
        let importance = result.nodes[0].importance;
        // access_factor after 365 days ≈ 0.5^(365/30) ≈ tiny
        // raw = 0.9 * tiny ≈ tiny, but floor = 0.3 wins
        assert!(
            (importance - 0.3_f32).abs() < 1e-4,
            "floor should clamp importance to 0.3, got {importance}"
        );
    }

    #[test]
    fn superseded_node_gets_half_multiplier() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // n1 supersedes n2  →  n2 is superseded
        graph.nodes.push(make_node_at("n1", 0.8, now, 0));
        graph.nodes.push(make_node_at("n2", 0.8, now, 0));
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n2".to_string(),
            relation: MemoryRelation::Supersedes,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 2);
        let n1 = result.nodes.iter().find(|n| n.id == "n1").unwrap();
        let n2 = result.nodes.iter().find(|n| n.id == "n2").unwrap();

        // n1: 0.8 * 1.0 * 1.0 = 0.8
        assert!((n1.importance - 0.8_f32).abs() < 1e-4, "n1 should be ~0.8");
        // n2: 0.8 * 1.0 * 0.5 = 0.4
        assert!(
            (n2.importance - 0.4_f32).abs() < 1e-4,
            "n2 (superseded) should be ~0.4, got {}",
            n2.importance
        );
    }

    #[test]
    fn contradicted_node_gets_quarter_multiplier() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // n1 contradicts n2  →  n2 is contradicted
        graph.nodes.push(make_node_at("n1", 0.8, now, 0));
        graph.nodes.push(make_node_at("n2", 0.8, now, 0));
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n2".to_string(),
            relation: MemoryRelation::Contradicts,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 2);
        let n1 = result.nodes.iter().find(|n| n.id == "n1").unwrap();
        let n2 = result.nodes.iter().find(|n| n.id == "n2").unwrap();

        // n1: 0.8 * 1.0 (access) * 1.0 (supersed) * 1.0 (contradicted) = 0.8
        assert!((n1.importance - 0.8_f32).abs() < 1e-4, "n1 should be ~0.8");
        // n2: 0.8 * 1.0 * 1.0 * 0.25 = 0.2
        assert!(
            (n2.importance - 0.2_f32).abs() < 1e-4,
            "n2 (contradicted) should be ~0.2, got {}",
            n2.importance
        );
    }

    #[test]
    fn contradicted_and_superseded_stacks_multiplicatively() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // n1 supersedes n3  AND  n2 contradicts n3  →  n3 gets both multipliers
        graph.nodes.push(make_node_at("n1", 0.8, now, 0));
        graph.nodes.push(make_node_at("n2", 0.8, now, 0));
        graph.nodes.push(make_node_at("n3", 0.8, now, 0));
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n3".to_string(),
            relation: MemoryRelation::Supersedes,
        });
        graph.edges.push(MemoryEdge {
            source_id: "n2".to_string(),
            target_id: "n3".to_string(),
            relation: MemoryRelation::Contradicts,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        let n3 = result.nodes.iter().find(|n| n.id == "n3").unwrap();
        // n3: 0.8 * 1.0 (access) * 0.5 (supersed) * 0.25 (contradicted) = 0.1
        assert!(
            (n3.importance - 0.1_f32).abs() < 1e-4,
            "n3 (superseded AND contradicted) should be ~0.1, got {}",
            n3.importance
        );
    }

    #[test]
    fn contradicted_node_still_respects_access_floor() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // access_count=20 → floor = min(0.3, 20*0.02) = 0.3
        graph.nodes.push(make_node_at("n1", 0.9, now, 20));
        graph.nodes.push(make_node_at("n2", 0.9, now, 20));
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n2".to_string(),
            relation: MemoryRelation::Contradicts,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        let n2 = result.nodes.iter().find(|n| n.id == "n2").unwrap();
        // Raw new_importance = 0.9 * 1.0 * 1.0 * 0.25 = 0.225
        // Access floor = 0.3 → floor wins
        assert!(
            (n2.importance - 0.3_f32).abs() < 1e-4,
            "access floor should clamp contradicted n2 to 0.3, got {}",
            n2.importance
        );
    }

    #[test]
    fn nodes_below_threshold_are_pruned() {
        let now = OffsetDateTime::now_utc();
        let very_old = now - Duration::days(365);
        let mut graph = empty_graph();
        // Very low importance + very old + no access floor → should drop below 0.05
        graph.nodes.push(make_node_at("tiny", 0.04, very_old, 0));
        // A healthy node that should survive
        graph.nodes.push(make_node_at("healthy", 0.8, now, 0));
        // Edge from healthy → tiny
        graph.edges.push(MemoryEdge {
            source_id: "healthy".to_string(),
            target_id: "tiny".to_string(),
            relation: MemoryRelation::RelatesTo,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1, "tiny node should be pruned");
        assert_eq!(result.nodes[0].id, "healthy");
        assert!(
            result.edges.is_empty(),
            "edge to pruned node should be removed"
        );
    }

    #[test]
    fn last_decay_run_is_updated() {
        let now = OffsetDateTime::now_utc();
        let graph = empty_graph();
        assert!(graph.last_decay_run.is_none());

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(
            result.last_decay_run,
            Some(now),
            "last_decay_run should be set to now"
        );
        assert_eq!(result.updated_at, now, "updated_at should be set to now");
    }

    // -----------------------------------------------------------------------
    // Grounding → decay coupling
    // -----------------------------------------------------------------------

    #[test]
    fn confirmed_grounding_raises_floor() {
        let now = OffsetDateTime::now_utc();
        let very_old = now - Duration::days(365);
        let mut graph = empty_graph();
        // Old node, no access floor (access_count 0): raw decays to ~0.
        let mut node = make_node_at("n1", 0.8, very_old, 0);
        node.last_grounding = Some(GroundingStatus::Confirmed);
        graph.nodes.push(node);

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1);
        let importance = result.nodes[0].importance;
        // Confirmed grounding floor (0.4) clamps the otherwise-tiny value.
        assert!(
            (importance - 0.4_f32).abs() < 1e-4,
            "Confirmed grounding floor should clamp to 0.4, got {importance}"
        );
    }

    #[test]
    fn contradicted_grounding_applies_half_multiplier() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // Fresh node (access_factor 1.0), no access floor.
        let mut contradicted = make_node_at("contradicted", 0.8, now, 0);
        contradicted.last_grounding = Some(GroundingStatus::Contradicted);
        graph.nodes.push(contradicted);
        // Neutral control with no grounding snapshot.
        graph.nodes.push(make_node_at("control", 0.8, now, 0));

        let result = MemoryDecayEngine::decay(&graph, now);

        let c = result
            .nodes
            .iter()
            .find(|n| n.id == "contradicted")
            .unwrap();
        let ctrl = result.nodes.iter().find(|n| n.id == "control").unwrap();
        // contradicted: 0.8 * 1.0 * 0.5 = 0.4
        assert!(
            (c.importance - 0.4_f32).abs() < 1e-4,
            "Contradicted grounding should halve importance to 0.4, got {}",
            c.importance
        );
        // control: 0.8 * 1.0 = 0.8
        assert!(
            (ctrl.importance - 0.8_f32).abs() < 1e-4,
            "neutral control should stay ~0.8, got {}",
            ctrl.importance
        );
    }

    #[test]
    fn grounding_neutral_for_unverifiable_unchecked_none() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        let mut unverifiable = make_node_at("unverifiable", 0.8, now, 0);
        unverifiable.last_grounding = Some(GroundingStatus::Unverifiable);
        let mut unchecked = make_node_at("unchecked", 0.8, now, 0);
        unchecked.last_grounding = Some(GroundingStatus::Unchecked);
        let none = make_node_at("none", 0.8, now, 0); // last_grounding = None
        graph.nodes.push(unverifiable);
        graph.nodes.push(unchecked);
        graph.nodes.push(none);

        let result = MemoryDecayEngine::decay(&graph, now);

        for id in ["unverifiable", "unchecked", "none"] {
            let n = result.nodes.iter().find(|n| n.id == id).unwrap();
            // No floor raised, no multiplier applied: 0.8 * 1.0 = 0.8.
            assert!(
                (n.importance - 0.8_f32).abs() < 1e-4,
                "{id} should be neutral (~0.8), got {}",
                n.importance
            );
        }
    }

    #[test]
    fn grounding_multiplier_composes_with_edges() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // n1 contradicts n2 (edge), AND n2 has a Contradicted grounding snapshot.
        graph.nodes.push(make_node_at("n1", 0.8, now, 0));
        let mut n2 = make_node_at("n2", 0.8, now, 0);
        n2.last_grounding = Some(GroundingStatus::Contradicted);
        graph.nodes.push(n2);
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n2".to_string(),
            relation: MemoryRelation::Contradicts,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        let n2 = result.nodes.iter().find(|n| n.id == "n2").unwrap();
        // n2: 0.8 * 1.0 (access) * 0.25 (contradicted edge) * 0.5 (grounding) = 0.1
        assert!(
            (n2.importance - 0.1_f32).abs() < 1e-4,
            "grounding multiplier should compose with the Contradicts edge to ~0.1, got {}",
            n2.importance
        );
    }

    #[test]
    fn contradicted_grounding_respects_access_floor() {
        let now = OffsetDateTime::now_utc();
        let very_old = now - Duration::days(365);
        let mut graph = empty_graph();
        // High access_count → floor 0.3; old so raw decays to ~0 even before the
        // 0.5 grounding multiplier. Floor must win (no auto-delete).
        let mut node = make_node_at("n1", 0.9, very_old, 20);
        node.last_grounding = Some(GroundingStatus::Contradicted);
        graph.nodes.push(node);

        let result = MemoryDecayEngine::decay(&graph, now);

        assert_eq!(result.nodes.len(), 1, "node must survive — no auto-delete");
        let importance = result.nodes[0].importance;
        assert!(
            (importance - 0.3_f32).abs() < 1e-4,
            "access floor (0.3) should clamp contradicted node, got {importance}"
        );
    }

    #[test]
    fn grounding_decay_is_deterministic() {
        let now = OffsetDateTime::now_utc();
        let old = now - Duration::days(45);
        let mut graph = empty_graph();
        let mut confirmed = make_node_at("confirmed", 0.8, old, 3);
        confirmed.last_grounding = Some(GroundingStatus::Confirmed);
        let mut contradicted = make_node_at("contradicted", 0.7, old, 3);
        contradicted.last_grounding = Some(GroundingStatus::Contradicted);
        graph.nodes.push(confirmed);
        graph.nodes.push(contradicted);
        graph.nodes.push(make_node_at("neutral", 0.6, old, 3));

        // Pure function of (graph, now): two runs on the *same input* match.
        let a = MemoryDecayEngine::decay(&graph, now);
        let b = MemoryDecayEngine::decay(&graph, now);
        assert_eq!(a, b, "decay must be deterministic for identical input");
    }

    #[test]
    fn confirmed_floor_overrides_supersedes_edge() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // n1 supersedes n2 (edge → 0.5x), but n2's anchors still resolve
        // (Confirmed → floor 0.4). The floor must win over the edge multiplier.
        graph.nodes.push(make_node_at("n1", 0.6, now, 0));
        let mut n2 = make_node_at("n2", 0.6, now, 0);
        n2.last_grounding = Some(GroundingStatus::Confirmed);
        graph.nodes.push(n2);
        graph.edges.push(MemoryEdge {
            source_id: "n1".to_string(),
            target_id: "n2".to_string(),
            relation: MemoryRelation::Supersedes,
        });

        let result = MemoryDecayEngine::decay(&graph, now);

        let n2 = result.nodes.iter().find(|n| n.id == "n2").unwrap();
        // raw = 0.6 * 1.0 (access) * 0.5 (supersedes) * 1.0 (grounding mult) = 0.3;
        // grounding floor 0.4 clamps above it.
        assert!(
            (n2.importance - 0.4_f32).abs() < 1e-4,
            "Confirmed floor (0.4) should override the Supersedes 0.5x edge, got {}",
            n2.importance
        );
    }

    #[test]
    fn contradicted_grounding_still_prunable_below_threshold() {
        let now = OffsetDateTime::now_utc();
        let mut graph = empty_graph();
        // Fresh, no access floor, importance just below 0.1. The 0.5x
        // Contradicted multiplier drops "doomed" under the 0.05 prune threshold
        // — no auto-delete logic, but also no special protection. The identical
        // neutral "survivor" proves the multiplier is what causes the prune.
        let mut doomed = make_node_at("doomed", 0.08, now, 0);
        doomed.last_grounding = Some(GroundingStatus::Contradicted);
        graph.nodes.push(doomed);
        graph.nodes.push(make_node_at("survivor", 0.08, now, 0));

        let result = MemoryDecayEngine::decay(&graph, now);

        // doomed: 0.08 * 1.0 * 0.5 = 0.04 < 0.05 → pruned.
        // survivor: 0.08 * 1.0 = 0.08 ≥ 0.05 → kept.
        assert_eq!(result.nodes.len(), 1, "only the survivor should remain");
        assert_eq!(result.nodes[0].id, "survivor");
    }

    // --- Clock-forward checks of the advertised 30-day half-life -----------

    #[test]
    fn single_run_halves_importance_after_thirty_days() {
        let t0 = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap(); // 2026-01-01Z
        let mut graph = empty_graph();
        graph.nodes.push(make_node_at("old", 0.8, t0, 0));
        graph
            .nodes
            .push(make_node_at("new", 0.8, t0 + Duration::days(30), 0));

        let out = MemoryDecayEngine::decay(&graph, t0 + Duration::days(30));
        let imp = |id: &str| out.nodes.iter().find(|n| n.id == id).unwrap().importance;
        assert!((imp("old") - 0.4).abs() < 1e-4, "old = {}", imp("old"));
        assert!((imp("new") - 0.8).abs() < 1e-4, "new = {}", imp("new"));
    }

    /// Regression: decay used to compound across runs (each run re-applied the
    /// full time since last access). Runs at day 30 and day 31 must leave
    /// `0.8 * 0.5^(31/30) ≈ 0.39`, not `≈ 0.19`.
    #[test]
    fn repeated_runs_respect_thirty_day_half_life() {
        let t0 = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut graph = empty_graph();
        graph.nodes.push(make_node_at("n", 0.8, t0, 0));

        let g1 = MemoryDecayEngine::decay(&graph, t0 + Duration::days(30));
        let g2 = MemoryDecayEngine::decay(&g1, t0 + Duration::days(31));
        let expected = 0.8 * 0.5_f32.powf(31.0 / 30.0);
        let got = g2.nodes[0].importance;
        assert!(
            (got - expected).abs() < 1e-3,
            "after runs at day 30 and 31: expected {expected:.4}, got {got:.4}"
        );
    }

    #[test]
    fn hourly_runs_over_thirty_days_halve_importance() {
        let t0 = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut g = empty_graph();
        g.nodes.push(make_node_at("n", 0.8, t0, 0));
        for h in 1..=(30 * 24) {
            g = MemoryDecayEngine::decay(&g, t0 + Duration::hours(h));
        }
        let ratio = g.nodes[0].importance / 0.8;
        assert!(
            (ratio - 0.5).abs() < 1e-3,
            "ratio after 720 hourly runs = {ratio}"
        );
    }

    #[test]
    fn legacy_node_without_last_decayed_at_uses_graph_last_decay_run() {
        // memory.json written before `lastDecayedAt` existed: the node was
        // already decayed through day 30 (graph.last_decay_run). A run at day
        // 31 must apply only one more day, not 31.
        let t0 = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut g = empty_graph();
        g.nodes.push(make_node_at("n", 0.4, t0, 0)); // already halved
        g.last_decay_run = Some(t0 + Duration::days(30));
        let out = MemoryDecayEngine::decay(&g, t0 + Duration::days(31));
        let expected = 0.4 * 0.5_f32.powf(1.0 / 30.0);
        assert!((out.nodes[0].importance - expected).abs() < 1e-4);
        assert_eq!(out.nodes[0].last_decayed_at, Some(t0 + Duration::days(31)));
    }

    #[test]
    fn legacy_memory_json_without_field_loads() {
        let json = r#"{"id":"n","content":"c","type":"PATTERN","importance":0.5,
            "created":"2026-01-01T00:00:00Z","lastAccessed":"2026-01-01T00:00:00Z",
            "accessCount":0,"anchors":[],"tags":[],"source":"EXPLICIT","provenance":"t"}"#;
        let node: repolens_core::model::memory::MemoryNode =
            serde_json::from_str(json).expect("legacy node without lastDecayedAt must deserialize");
        assert_eq!(node.last_decayed_at, None);
        let out = serde_json::to_string(&node).unwrap();
        assert!(
            !out.contains("lastDecayedAt"),
            "None is not serialized: {out}"
        );
    }

    #[test]
    fn access_after_last_decay_restarts_the_clock() {
        let t0 = OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap();
        let mut g = empty_graph();
        let mut n = make_node_at("n", 0.8, t0 + Duration::days(10), 1);
        n.last_decayed_at = Some(t0);
        g.nodes.push(n);
        let out = MemoryDecayEngine::decay(&g, t0 + Duration::days(40));
        let expected = 0.8 * 0.5_f32; // 30 days since access, not 40
        assert!((out.nodes[0].importance - expected).abs() < 1e-4);
    }

    /// #14: decay tracks the importance the node would have without the
    /// grounding penalty, compounding across runs like the penalty does.
    #[test]
    fn tracks_importance_before_grounding_across_runs() {
        let t0 = OffsetDateTime::now_utc();
        let mut g = empty_graph();
        let mut n = make_node_at("n", 0.5, t0, 0);
        n.last_grounding = Some(GroundingStatus::Contradicted);
        g.nodes.push(n);

        let once = MemoryDecayEngine::decay(&g, t0 + Duration::days(30));
        let n = &once.nodes[0];
        assert!((n.importance - 0.125).abs() < 1e-4, "{}", n.importance);
        let before = n.importance_before_grounding.expect("penalty applied");
        assert!((before - 0.25).abs() < 1e-4, "{before}");

        // A second run an hour later halves again; the untouched value
        // only sees the hour of time decay.
        let twice = MemoryDecayEngine::decay(&once, t0 + Duration::days(30) + Duration::hours(2));
        let n = &twice.nodes[0];
        let before = n.importance_before_grounding.unwrap();
        assert!((n.importance / before - 0.25).abs() < 1e-3, "{n:?}");
    }

    /// #14: when a floor lifts the penalised value to the same place as
    /// the unpenalised one, there's no penalty left to report.
    #[test]
    fn floor_that_cancels_the_penalty_clears_it() {
        let now = OffsetDateTime::now_utc();
        let mut g = empty_graph();
        // 120 days unused: with or without the penalty the value is below
        // the access floor (15 accesses), so both end at that floor.
        let mut n = make_node_at("n", 0.5, now - Duration::days(120), 15);
        n.last_grounding = Some(GroundingStatus::Contradicted);
        g.nodes.push(n);
        let out = MemoryDecayEngine::decay(&g, now);
        assert!((out.nodes[0].importance - 0.3).abs() < 1e-4);
        assert_eq!(out.nodes[0].importance_before_grounding, None);
    }

    /// Nodes the penalty never touched carry no extra value.
    #[test]
    fn unpenalised_nodes_have_no_before_grounding_value() {
        let now = OffsetDateTime::now_utc();
        let mut g = empty_graph();
        g.nodes
            .push(make_node_at("n", 0.8, now - Duration::days(10), 0));
        let out = MemoryDecayEngine::decay(&g, now);
        assert_eq!(out.nodes[0].importance_before_grounding, None);
    }
}
