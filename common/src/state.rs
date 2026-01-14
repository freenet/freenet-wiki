//! Top-level wiki state using the composable pattern.

use crate::contributor::{ContributorId, ContributorsDelta, ContributorsV1};
use crate::page::{PageDelta, PagePath, PageSummary, WikiPageV1};
use crate::util::{sign_struct, verify_struct};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Contract parameters - fixed at creation, determines contract identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiParameters {
    /// The wiki owner's public key
    pub owner: VerifyingKey,
    /// Unique wiki identifier
    pub wiki_id: [u8; 32],
}

impl WikiParameters {
    /// Get the owner's contributor ID.
    pub fn owner_id(&self) -> ContributorId {
        ContributorId::from(&self.owner)
    }
}

/// Top-level wiki state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiStateV1 {
    /// Wiki configuration (owner-only)
    pub config: WikiConfigV1,
    /// Invited contributors
    pub contributors: ContributorsV1,
    /// All wiki pages
    pub pages: WikiPagesV1,
}

impl Default for WikiStateV1 {
    fn default() -> Self {
        Self {
            config: WikiConfigV1::default(),
            contributors: ContributorsV1::default(),
            pages: WikiPagesV1::default(),
        }
    }
}

impl WikiStateV1 {
    /// Create a new wiki with initial configuration.
    pub fn new(config: WikiConfig, owner_key: &SigningKey) -> Self {
        Self {
            config: WikiConfigV1::new(config, owner_key),
            contributors: ContributorsV1::default(),
            pages: WikiPagesV1::default(),
        }
    }

    /// Verify the entire state.
    pub fn verify(&self, params: &WikiParameters) -> Result<(), String> {
        // Verify config
        self.config.verify(params)?;

        // Verify contributors
        self.contributors_valid(params)?;

        // Verify pages
        self.pages.verify(self, params)?;

        Ok(())
    }

    /// Check if a contributor is authorized.
    pub fn is_authorized(&self, id: &ContributorId, params: &WikiParameters) -> bool {
        self.contributors.is_authorized(id, &params.owner_id())
    }

    /// Get verifying key for a contributor.
    pub fn get_contributor_vk(
        &self,
        id: &ContributorId,
        params: &WikiParameters,
    ) -> Option<VerifyingKey> {
        self.contributors.get_verifying_key(id, &params.owner)
    }

    fn contributors_valid(&self, _params: &WikiParameters) -> Result<(), String> {
        if self.contributors.contributors.len() > self.config.config.max_contributors {
            return Err("Too many contributors".to_string());
        }
        Ok(())
    }
}

/// Wiki configuration (signed by owner).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiConfigV1 {
    pub config: WikiConfig,
    pub signature: Signature,
}

impl Default for WikiConfigV1 {
    fn default() -> Self {
        Self {
            config: WikiConfig::default(),
            signature: Signature::from_bytes(&[0u8; 64]),
        }
    }
}

impl WikiConfigV1 {
    /// Create a new signed config.
    pub fn new(config: WikiConfig, owner_key: &SigningKey) -> Self {
        Self {
            signature: sign_struct(&config, owner_key),
            config,
        }
    }

    /// Verify the config signature.
    pub fn verify(&self, params: &WikiParameters) -> Result<(), String> {
        verify_struct(&self.config, &self.signature, &params.owner)
            .map_err(|e| format!("Invalid config signature: {}", e))
    }
}

/// Wiki configuration data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiConfig {
    /// Configuration version (increments on update)
    pub version: u32,
    /// Wiki name
    pub name: String,
    /// Wiki description
    pub description: Option<String>,
    /// Maximum number of pages
    pub max_pages: usize,
    /// Maximum page content size (bytes)
    pub max_page_size: usize,
    /// Maximum pending patches per page
    pub max_patches_per_page: usize,
    /// Maximum contributors
    pub max_contributors: usize,
}

impl Default for WikiConfig {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled Wiki".to_string(),
            description: None,
            max_pages: 1000,
            max_page_size: 1024 * 1024, // 1MB
            max_patches_per_page: 100,
            max_contributors: 100,
        }
    }
}

/// Container for all wiki pages.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct WikiPagesV1 {
    /// Pages indexed by path (BTreeMap for deterministic ordering)
    pub pages: BTreeMap<PagePath, WikiPageV1>,
}

impl WikiPagesV1 {
    /// Get a page by path.
    pub fn get(&self, path: &PagePath) -> Option<&WikiPageV1> {
        self.pages.get(path)
    }

    /// Get a mutable page by path.
    pub fn get_mut(&mut self, path: &PagePath) -> Option<&mut WikiPageV1> {
        self.pages.get_mut(path)
    }

    /// Add a new page.
    pub fn add_page(&mut self, page: WikiPageV1, max_pages: usize) -> Result<(), String> {
        if self.pages.len() >= max_pages {
            return Err("Maximum pages reached".to_string());
        }
        if self.pages.contains_key(&page.path) {
            return Err(format!("Page '{}' already exists", page.path.as_str()));
        }
        self.pages.insert(page.path.clone(), page);
        Ok(())
    }

    /// Verify all pages.
    pub fn verify(&self, state: &WikiStateV1, params: &WikiParameters) -> Result<(), String> {
        if self.pages.len() > state.config.config.max_pages {
            return Err("Too many pages".to_string());
        }

        for (path, page) in &self.pages {
            // Verify page path matches
            if &page.path != path {
                return Err(format!("Page path mismatch: {} vs {}", page.path.as_str(), path.as_str()));
            }

            // Verify revision signature
            let author_vk = state
                .get_contributor_vk(&page.revision.revision.author, params)
                .ok_or_else(|| {
                    format!(
                        "Revision author {:?} not authorized",
                        page.revision.revision.author
                    )
                })?;
            page.revision.verify(&author_vk)?;

            // Verify each patch
            for patch in &page.patches {
                let patch_author_vk = state
                    .get_contributor_vk(&patch.patch.author, params)
                    .ok_or_else(|| {
                        format!("Patch author {:?} not authorized", patch.patch.author)
                    })?;
                patch.verify(&patch_author_vk)?;
            }

            // Check size limits
            if page.revision.revision.content.len() > state.config.config.max_page_size {
                return Err(format!("Page '{}' exceeds max size", path.as_str()));
            }
            if page.patches.len() > state.config.config.max_patches_per_page {
                return Err(format!("Page '{}' has too many patches", path.as_str()));
            }
        }

        Ok(())
    }
}

// ============================================================================
// Summary and Delta types for sync
// ============================================================================

/// Summary of wiki state for sync.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct WikiStateSummary {
    pub config_version: u32,
    pub contributor_ids: std::collections::HashSet<ContributorId>,
    pub pages: BTreeMap<PagePath, PageSummary>,
}

impl WikiStateV1 {
    /// Generate summary for sync.
    pub fn summarize(&self) -> WikiStateSummary {
        WikiStateSummary {
            config_version: self.config.config.version,
            contributor_ids: self.contributors.summarize(),
            pages: self
                .pages
                .pages
                .iter()
                .map(|(path, page)| (path.clone(), page.summarize()))
                .collect(),
        }
    }
}

/// Delta for wiki state updates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiStateDelta {
    pub config: Option<WikiConfigV1>,
    pub contributors: Option<ContributorsDelta>,
    pub pages: Option<WikiPagesDelta>,
}

/// Delta for pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WikiPagesDelta {
    pub updates: BTreeMap<PagePath, PageDelta>,
}

impl WikiStateV1 {
    /// Compute delta from old summary.
    pub fn delta(&self, old_summary: &WikiStateSummary) -> Option<WikiStateDelta> {
        let config = if self.config.config.version > old_summary.config_version {
            Some(self.config.clone())
        } else {
            None
        };

        let contributors = self.contributors.delta(&old_summary.contributor_ids);

        let mut page_updates = BTreeMap::new();
        for (path, page) in &self.pages.pages {
            if let Some(old_page_summary) = old_summary.pages.get(path) {
                if let Some(delta) = page.compute_delta(old_page_summary) {
                    page_updates.insert(path.clone(), delta);
                }
            } else {
                // New page
                page_updates.insert(path.clone(), PageDelta::NewPage(page.clone()));
            }
        }

        let pages = if page_updates.is_empty() {
            None
        } else {
            Some(WikiPagesDelta {
                updates: page_updates,
            })
        };

        if config.is_none() && contributors.is_none() && pages.is_none() {
            None
        } else {
            Some(WikiStateDelta {
                config,
                contributors,
                pages,
            })
        }
    }

    /// Apply a delta.
    pub fn apply_delta(
        &mut self,
        delta: &WikiStateDelta,
        params: &WikiParameters,
    ) -> Result<(), String> {
        // Apply config update
        if let Some(new_config) = &delta.config {
            new_config.verify(params)?;
            if new_config.config.version > self.config.config.version {
                self.config = new_config.clone();
            }
        }

        // Apply contributor updates
        if let Some(contrib_delta) = &delta.contributors {
            self.contributors.apply_delta(
                contrib_delta,
                &params.owner,
                self.config.config.max_contributors,
            )?;
        }

        // Apply page updates
        if let Some(pages_delta) = &delta.pages {
            for (path, page_delta) in &pages_delta.updates {
                match page_delta {
                    PageDelta::NewPage(page) => {
                        // Verify the new page
                        let author_vk = self
                            .get_contributor_vk(&page.revision.revision.author, params)
                            .ok_or_else(|| "Page author not authorized".to_string())?;
                        page.revision.verify(&author_vk)?;

                        self.pages
                            .add_page(page.clone(), self.config.config.max_pages)?;
                    }
                    PageDelta::Updates {
                        new_revision,
                        new_patches,
                    } => {
                        // Check page exists
                        if !self.pages.pages.contains_key(path) {
                            return Err(format!("Page '{}' not found", path.as_str()));
                        }

                        // Verify revision (borrowing self immutably)
                        let verified_revision = if let Some(rev) = new_revision {
                            let author_vk = self
                                .get_contributor_vk(&rev.revision.author, params)
                                .ok_or_else(|| "Revision author not authorized".to_string())?;
                            rev.verify(&author_vk)?;
                            Some(rev.clone())
                        } else {
                            None
                        };

                        // Verify all patches (borrowing self immutably)
                        let mut verified_patches = Vec::new();
                        for patch in new_patches {
                            let author_vk = self
                                .get_contributor_vk(&patch.patch.author, params)
                                .ok_or_else(|| "Patch author not authorized".to_string())?;
                            patch.verify(&author_vk)?;
                            verified_patches.push(patch.clone());
                        }

                        // Now apply mutations (borrowing self.pages mutably)
                        let max_patches = self.config.config.max_patches_per_page;
                        let page = self.pages.get_mut(path).unwrap();

                        if let Some(rev) = verified_revision {
                            page.apply_revision(rev)?;
                        }

                        for patch in verified_patches {
                            page.add_patch(patch, max_patches)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    fn generate_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn test_create_wiki() {
        let owner_key = generate_key();
        let config = WikiConfig {
            name: "My Wiki".to_string(),
            ..Default::default()
        };

        let wiki = WikiStateV1::new(config, &owner_key);

        assert_eq!(wiki.config.config.name, "My Wiki");
        assert!(wiki.contributors.contributors.is_empty());
        assert!(wiki.pages.pages.is_empty());
    }

    #[test]
    fn test_add_page() {
        let owner_key = generate_key();
        let params = WikiParameters {
            owner: owner_key.verifying_key(),
            wiki_id: [0u8; 32],
        };

        let mut wiki = WikiStateV1::new(WikiConfig::default(), &owner_key);

        let page = WikiPageV1::new(
            PagePath::normalize("home"),
            "# Welcome".to_string(),
            &owner_key,
        );

        wiki.pages.add_page(page, 1000).unwrap();

        assert!(wiki.pages.get(&PagePath::normalize("home")).is_some());
        assert!(wiki.verify(&params).is_ok());
    }

    #[test]
    fn test_delta_sync() {
        let owner_key = generate_key();
        let params = WikiParameters {
            owner: owner_key.verifying_key(),
            wiki_id: [0u8; 32],
        };

        let mut wiki = WikiStateV1::new(WikiConfig::default(), &owner_key);

        // Get initial summary
        let summary1 = wiki.summarize();

        // Add a page
        let page = WikiPageV1::new(
            PagePath::normalize("home"),
            "# Welcome".to_string(),
            &owner_key,
        );
        wiki.pages.add_page(page, 1000).unwrap();

        // Compute delta
        let delta = wiki.delta(&summary1).expect("Should have delta");

        // Apply to fresh wiki
        let mut wiki2 = WikiStateV1::new(WikiConfig::default(), &owner_key);
        wiki2.apply_delta(&delta, &params).unwrap();

        // Should have the page
        assert!(wiki2.pages.get(&PagePath::normalize("home")).is_some());
    }
}
