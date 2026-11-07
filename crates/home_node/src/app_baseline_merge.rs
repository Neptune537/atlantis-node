//! # App 3-Way Baseline Merge Engine (`home_node::app_baseline_merge`)
//!
//! Clean-room Rust implementation of the 3-way baseline tree merge engine.
//! Allows a Club Home Node or edge client to customize app files and configuration
//! (via local agent modifications) while retaining the capability to cleanly merge
//! upstream `.nautpak` store releases without clobbering user modifications.
//!
//! Evaluates three trees:
//! - `Baseline`: What the app was when installed or last upgraded
//! - `Mine`: The local customized version currently in the workspace
//! - `Theirs`: The incoming upstream update package
//!
//! Rules:
//! 1. File unchanged since baseline on local side, changed upstream -> Take upstream
//! 2. File changed locally, unchanged upstream -> Keep local version
//! 3. File changed identically on both sides -> Merge cleanly
//! 4. File changed conflictingly -> Flag conflict with configured resolution strategy

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// Tree representation mapping relative file paths to content bytes.
pub type FileTree = BTreeMap<String, Vec<u8>>;

/// Conflict resolution strategy when text merges collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeResolutionStrategy {
    KeepMine,
    TakeTheirs,
    WriteConflictMarkers,
}

/// A conflict identified during 3-way reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConflict {
    pub file_path: String,
    pub reason: String,
}

/// The result of performing a 3-way tree merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeMergeResult {
    /// Merged output files. None indicates file deletion.
    pub files: BTreeMap<String, Option<Vec<u8>>>,
    pub cleanly_merged_files: Vec<String>,
    pub conflicts: Vec<MergeConflict>,
}

/// Performs a 3-way merge between `baseline`, `mine` (local changes), and `theirs` (upstream update).
pub fn merge_3way_trees(
    baseline: &FileTree,
    mine: &FileTree,
    theirs: &FileTree,
    strategy: MergeResolutionStrategy,
) -> TreeMergeResult {
    let mut all_paths = HashSet::new();
    for p in baseline.keys() {
        all_paths.insert(p.clone());
    }
    for p in mine.keys() {
        all_paths.insert(p.clone());
    }
    for p in theirs.keys() {
        all_paths.insert(p.clone());
    }

    let mut result_files = BTreeMap::new();
    let mut cleanly_merged = Vec::new();
    let mut conflicts = Vec::new();

    for path in all_paths {
        let b = baseline.get(&path);
        let m = mine.get(&path);
        let t = theirs.get(&path);

        match (b, m, t) {
            // Case 1: Identical on all sides
            (Some(bv), Some(mv), Some(tv)) if bv == mv && mv == tv => {
                result_files.insert(path, Some(mv.clone()));
            }
            // Case 2: Unchanged locally, updated upstream -> Take upstream
            (Some(bv), Some(mv), Some(tv)) if bv == mv && mv != tv => {
                result_files.insert(path.clone(), Some(tv.clone()));
                cleanly_merged.push(path);
            }
            // Case 3: Changed locally, unchanged upstream -> Keep mine
            (Some(bv), Some(mv), Some(tv)) if bv != mv && bv == tv => {
                result_files.insert(path.clone(), Some(mv.clone()));
                cleanly_merged.push(path);
            }
            // Case 4: Changed on both sides identically
            (Some(_), Some(mv), Some(tv)) if mv == tv => {
                result_files.insert(path.clone(), Some(mv.clone()));
                cleanly_merged.push(path);
            }
            // Case 5: Both modified differently -> Conflict
            (Some(_), Some(mv), Some(tv)) => {
                conflicts.push(MergeConflict {
                    file_path: path.clone(),
                    reason: "Simultaneously modified on both branches".to_string(),
                });
                match strategy {
                    MergeResolutionStrategy::KeepMine => {
                        result_files.insert(path, Some(mv.clone()));
                    }
                    MergeResolutionStrategy::TakeTheirs => {
                        result_files.insert(path, Some(tv.clone()));
                    }
                    MergeResolutionStrategy::WriteConflictMarkers => {
                        let conflict_text = format!(
                            "<<<<<<< MINE\n{}\n=======\n{}\n>>>>>>> THEIRS\n",
                            String::from_utf8_lossy(mv),
                            String::from_utf8_lossy(tv)
                        );
                        result_files.insert(path, Some(conflict_text.into_bytes()));
                    }
                }
            }
            // Case 6: Created new in upstream only
            (None, None, Some(tv)) => {
                result_files.insert(path.clone(), Some(tv.clone()));
                cleanly_merged.push(path);
            }
            // Case 7: Created new locally only
            (None, Some(mv), None) => {
                result_files.insert(path.clone(), Some(mv.clone()));
                cleanly_merged.push(path);
            }
            // Case 8: Deleted locally, unchanged upstream -> Remain deleted
            (Some(bv), None, Some(tv)) if bv == tv => {
                result_files.insert(path.clone(), None);
                cleanly_merged.push(path);
            }
            // Case 9: Deleted upstream, unchanged locally -> Accept deletion
            (Some(bv), Some(mv), None) if bv == mv => {
                result_files.insert(path.clone(), None);
                cleanly_merged.push(path);
            }
            // Other deletion / creation collisions
            (Some(_), Some(_), None) => {
                conflicts.push(MergeConflict {
                    file_path: path.clone(),
                    reason: "Modified locally but deleted upstream".to_string(),
                });
                result_files.insert(path, m.cloned());
            }
            (Some(_), None, Some(_)) => {
                conflicts.push(MergeConflict {
                    file_path: path.clone(),
                    reason: "Deleted locally but modified upstream".to_string(),
                });
                result_files.insert(path, t.cloned());
            }
            _ => {}
        }
    }

    cleanly_merged.sort();
    conflicts.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    TreeMergeResult {
        files: result_files,
        cleanly_merged_files: cleanly_merged,
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_3way_merge_retains_local_edits_and_takes_upstream_updates() {
        let mut baseline = FileTree::new();
        baseline.insert("config.json".into(), b"{\"theme\": \"dark\"}".to_vec());
        baseline.insert("hud.js".into(), b"function renderHud() {}".to_vec());

        // Local agent customized config.json
        let mut mine = FileTree::new();
        mine.insert("config.json".into(), b"{\"theme\": \"cyberpunk_cyan\"}".to_vec());
        mine.insert("hud.js".into(), b"function renderHud() {}".to_vec());

        // Upstream release updated hud.js with new radar features
        let mut theirs = FileTree::new();
        theirs.insert("config.json".into(), b"{\"theme\": \"dark\"}".to_vec());
        theirs.insert("hud.js".into(), b"function renderHud() { updateRadar(); }".to_vec());
        theirs.insert("radar.js".into(), b"function updateRadar() {}".to_vec());

        let res = merge_3way_trees(
            &baseline,
            &mine,
            &theirs,
            MergeResolutionStrategy::WriteConflictMarkers,
        );

        assert_eq!(res.conflicts.len(), 0);
        // config.json kept local customization
        assert_eq!(
            res.files.get("config.json").unwrap().as_ref().unwrap(),
            b"{\"theme\": \"cyberpunk_cyan\"}"
        );
        // hud.js accepted upstream improvements
        assert_eq!(
            res.files.get("hud.js").unwrap().as_ref().unwrap(),
            b"function renderHud() { updateRadar(); }"
        );
        // new file radar.js was added
        assert!(res.files.contains_key("radar.js"));
    }

    #[test]
    fn test_3way_merge_conflict_markers() {
        let mut baseline = FileTree::new();
        baseline.insert("store.json".into(), b"v1".to_vec());

        let mut mine = FileTree::new();
        mine.insert("store.json".into(), b"v1-local".to_vec());

        let mut theirs = FileTree::new();
        theirs.insert("store.json".into(), b"v1-upstream".to_vec());

        let res = merge_3way_trees(
            &baseline,
            &mine,
            &theirs,
            MergeResolutionStrategy::WriteConflictMarkers,
        );

        assert_eq!(res.conflicts.len(), 1);
        let content = String::from_utf8(res.files.get("store.json").unwrap().as_ref().unwrap().clone()).unwrap();
        assert!(content.contains("<<<<<<< MINE"));
        assert!(content.contains("v1-local"));
        assert!(content.contains(">>>>>>> THEIRS"));
        assert!(content.contains("v1-upstream"));
    }
}
