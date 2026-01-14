//! Wiki page with revision + patches model.
//!
//! Each page has a base revision (full content) and pending patches.
//! Patches can be committed to create a new revision.

use crate::contributor::ContributorId;
use crate::patch_ops::{apply_operations, PatchOp};
use crate::util::{fast_hash, sign_struct, verify_struct, FastHash};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Normalized page path (lowercase, no leading/trailing slashes).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PagePath(pub String);

impl PagePath {
    /// Normalize a path string.
    pub fn normalize(path: &str) -> Self {
        let normalized = path
            .to_lowercase()
            .trim_matches('/')
            .replace("//", "/")
            .to_string();

        PagePath(if normalized.is_empty() {
            "home".to_string()
        } else {
            normalized
        })
    }

    /// Get the path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Unique identifier for a patch, derived from its signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PatchId(pub FastHash);

/// A wiki page with base revision and pending patches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiPageV1 {
    /// The page path
    pub path: PagePath,
    /// The current base revision
    pub revision: SignedRevision,
    /// Pending patches against the revision
    pub patches: Vec<SignedPatch>,
}

impl WikiPageV1 {
    /// Create a new page with initial content.
    pub fn new(path: PagePath, content: String, author: &SigningKey) -> Self {
        let author_id = ContributorId::from(author.verifying_key());
        let revision = Revision {
            version: 1,
            content,
            author: author_id,
            created_at: Utc::now(),
        };
        let signed_revision = SignedRevision::new(revision, author);

        Self {
            path,
            revision: signed_revision,
            patches: Vec::new(),
        }
    }

    /// Render the page content with all patches applied.
    pub fn render(&self) -> String {
        // Collect operations from patches targeting current revision
        let mut ops_with_meta: Vec<(&PatchOp, DateTime<Utc>, PatchId)> = self
            .patches
            .iter()
            .filter(|p| p.patch.target_version == self.revision.revision.version)
            .flat_map(|p| {
                p.patch
                    .operations
                    .iter()
                    .map(move |op| (op, p.patch.created_at, p.id()))
            })
            .collect();

        // Sort by (timestamp, patch_id) for deterministic ordering
        ops_with_meta.sort_by_key(|(_, ts, id)| (*ts, *id));

        // Extract just the operations
        let ops: Vec<PatchOp> = ops_with_meta.into_iter().map(|(op, _, _)| op.clone()).collect();

        // Apply to base content
        apply_operations(&self.revision.revision.content, &ops)
    }

    /// Commit all patches into a new revision.
    pub fn commit(&self, committer: &SigningKey) -> SignedRevision {
        let committer_id = ContributorId::from(committer.verifying_key());
        let new_revision = Revision {
            version: self.revision.revision.version + 1,
            content: self.render(),
            author: committer_id,
            created_at: Utc::now(),
        };
        SignedRevision::new(new_revision, committer)
    }

    /// Apply a new revision (from commit).
    pub fn apply_revision(&mut self, new_revision: SignedRevision) -> Result<(), String> {
        if new_revision.revision.version <= self.revision.revision.version {
            return Err("New revision version must be greater than current".to_string());
        }

        self.revision = new_revision;

        // Remove patches targeting old versions
        self.patches
            .retain(|p| p.patch.target_version >= self.revision.revision.version);

        Ok(())
    }

    /// Add a new patch.
    pub fn add_patch(&mut self, patch: SignedPatch, max_patches: usize) -> Result<(), String> {
        // Check if targeting current version
        if patch.patch.target_version != self.revision.revision.version {
            return Err(format!(
                "Patch targets version {} but current is {}",
                patch.patch.target_version, self.revision.revision.version
            ));
        }

        // Skip duplicates
        if self.patches.iter().any(|p| p.id() == patch.id()) {
            return Ok(());
        }

        // Enforce limit
        if self.patches.len() >= max_patches {
            return Err("Maximum pending patches reached".to_string());
        }

        self.patches.push(patch);

        // Sort for deterministic ordering
        self.patches.sort_by_key(|p| (p.patch.created_at, p.id()));

        Ok(())
    }
}

/// A signed revision (full page content).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedRevision {
    pub revision: Revision,
    pub signature: Signature,
}

impl SignedRevision {
    /// Create a new signed revision.
    pub fn new(revision: Revision, signer: &SigningKey) -> Self {
        Self {
            signature: sign_struct(&revision, signer),
            revision,
        }
    }

    /// Verify the revision signature.
    pub fn verify(&self, signer_vk: &VerifyingKey) -> Result<(), String> {
        verify_struct(&self.revision, &self.signature, signer_vk)
            .map_err(|e| format!("Invalid revision signature: {}", e))
    }
}

/// A page revision (full content snapshot).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    /// Sequential version number (increments on commit)
    pub version: u64,
    /// Full markdown content
    pub content: String,
    /// Who created this revision
    pub author: ContributorId,
    /// When this revision was created
    pub created_at: DateTime<Utc>,
}

/// A signed patch against a revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedPatch {
    pub patch: Patch,
    pub signature: Signature,
}

impl SignedPatch {
    /// Create a new signed patch.
    pub fn new(patch: Patch, signer: &SigningKey) -> Self {
        Self {
            signature: sign_struct(&patch, signer),
            patch,
        }
    }

    /// Get the patch ID (hash of signature).
    pub fn id(&self) -> PatchId {
        PatchId(fast_hash(&self.signature.to_bytes()))
    }

    /// Verify the patch signature.
    pub fn verify(&self, signer_vk: &VerifyingKey) -> Result<(), String> {
        verify_struct(&self.patch, &self.signature, signer_vk)
            .map_err(|e| format!("Invalid patch signature: {}", e))
    }
}

/// A patch against a specific revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    /// Which revision this patch targets
    pub target_version: u64,
    /// Who created this patch
    pub author: ContributorId,
    /// When the patch was created
    pub created_at: DateTime<Utc>,
    /// The patch operations
    pub operations: Vec<PatchOp>,
    /// Optional description (commit message)
    pub message: Option<String>,
}

/// Summary of a page for sync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSummary {
    pub revision_version: u64,
    pub patch_ids: HashSet<PatchId>,
}

impl WikiPageV1 {
    /// Generate a summary for sync.
    pub fn summarize(&self) -> PageSummary {
        PageSummary {
            revision_version: self.revision.revision.version,
            patch_ids: self.patches.iter().map(|p| p.id()).collect(),
        }
    }
}

/// Delta for page updates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PageDelta {
    /// A completely new page
    NewPage(WikiPageV1),
    /// Updates to an existing page
    Updates {
        /// New revision (from commit)
        new_revision: Option<SignedRevision>,
        /// New patches
        new_patches: Vec<SignedPatch>,
    },
}

impl WikiPageV1 {
    /// Compute delta from old summary.
    pub fn compute_delta(&self, old_summary: &PageSummary) -> Option<PageDelta> {
        let mut new_revision = None;
        let mut new_patches = Vec::new();

        // Check if revision changed
        if self.revision.revision.version > old_summary.revision_version {
            new_revision = Some(self.revision.clone());
        }

        // Find new patches
        for patch in &self.patches {
            if !old_summary.patch_ids.contains(&patch.id()) {
                new_patches.push(patch.clone());
            }
        }

        if new_revision.is_none() && new_patches.is_empty() {
            None
        } else {
            Some(PageDelta::Updates {
                new_revision,
                new_patches,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch_ops::{delete_line, insert_after, replace_line};
    use rand::rngs::OsRng;

    fn generate_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn test_create_page() {
        let key = generate_key();
        let page = WikiPageV1::new(
            PagePath::normalize("home"),
            "# Welcome\n\nThis is the home page.".to_string(),
            &key,
        );

        assert_eq!(page.path.as_str(), "home");
        assert_eq!(page.revision.revision.version, 1);
        assert!(page.patches.is_empty());
    }

    #[test]
    fn test_render_with_patches() {
        let key = generate_key();
        let mut page = WikiPageV1::new(
            PagePath::normalize("test"),
            "line1\nline2\nline3".to_string(),
            &key,
        );

        let patch = Patch {
            target_version: 1,
            author: ContributorId::from(key.verifying_key()),
            created_at: Utc::now(),
            operations: vec![delete_line("line2")],
            message: None,
        };
        let signed_patch = SignedPatch::new(patch, &key);
        page.add_patch(signed_patch, 100).unwrap();

        let rendered = page.render();
        assert_eq!(rendered, "line1\nline3");
    }

    #[test]
    fn test_commit_patches() {
        let key = generate_key();
        let mut page = WikiPageV1::new(
            PagePath::normalize("test"),
            "line1\nline2".to_string(),
            &key,
        );

        // Add a patch
        let patch = Patch {
            target_version: 1,
            author: ContributorId::from(key.verifying_key()),
            created_at: Utc::now(),
            operations: vec![insert_after(Some("line1"), vec!["inserted".to_string()])],
            message: Some("Add line".to_string()),
        };
        let signed_patch = SignedPatch::new(patch, &key);
        page.add_patch(signed_patch, 100).unwrap();

        // Commit
        let new_revision = page.commit(&key);
        assert_eq!(new_revision.revision.version, 2);
        assert_eq!(new_revision.revision.content, "line1\ninserted\nline2");

        // Apply commit
        page.apply_revision(new_revision).unwrap();
        assert_eq!(page.revision.revision.version, 2);
        assert!(page.patches.is_empty()); // Patches cleared after commit
    }

    #[test]
    fn test_patches_merge_commutatively() {
        let key = generate_key();
        let base_content = "line1\nline2\nline3\nline4";

        // Create two pages with same base
        let mut page_ab = WikiPageV1::new(PagePath::normalize("test"), base_content.to_string(), &key);
        let mut page_ba = WikiPageV1::new(PagePath::normalize("test"), base_content.to_string(), &key);

        let author_id = ContributorId::from(key.verifying_key());

        // Patch A: delete line2
        let patch_a = Patch {
            target_version: 1,
            author: author_id,
            created_at: Utc::now(),
            operations: vec![delete_line("line2")],
            message: None,
        };
        let signed_a = SignedPatch::new(patch_a, &key);

        // Patch B: delete line3 (created slightly later)
        let patch_b = Patch {
            target_version: 1,
            author: author_id,
            created_at: Utc::now() + chrono::Duration::milliseconds(1),
            operations: vec![delete_line("line3")],
            message: None,
        };
        let signed_b = SignedPatch::new(patch_b, &key);

        // Apply A then B
        page_ab.add_patch(signed_a.clone(), 100).unwrap();
        page_ab.add_patch(signed_b.clone(), 100).unwrap();

        // Apply B then A
        page_ba.add_patch(signed_b, 100).unwrap();
        page_ba.add_patch(signed_a, 100).unwrap();

        // Both should render the same
        assert_eq!(page_ab.render(), page_ba.render());
        assert_eq!(page_ab.render(), "line1\nline4");
    }

    #[test]
    fn test_path_normalization() {
        assert_eq!(PagePath::normalize("Home").as_str(), "home");
        assert_eq!(PagePath::normalize("/docs/api/").as_str(), "docs/api");
        assert_eq!(PagePath::normalize("").as_str(), "home");
        assert_eq!(PagePath::normalize("//double//slash").as_str(), "double/slash");
    }
}
