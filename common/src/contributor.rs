//! Contributor management for wiki editing permissions.
//!
//! Similar to River's member system - owner can invite contributors,
//! contributors can invite others, creating a chain of trust.

use crate::util::{fast_hash, sign_struct, verify_struct, FastHash};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Unique identifier for a contributor, derived from their public key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContributorId(pub FastHash);

impl From<&VerifyingKey> for ContributorId {
    fn from(vk: &VerifyingKey) -> Self {
        ContributorId(fast_hash(&vk.to_bytes()))
    }
}

impl From<VerifyingKey> for ContributorId {
    fn from(vk: VerifyingKey) -> Self {
        ContributorId::from(&vk)
    }
}

/// A contributor with their invitation proof.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedContributor {
    pub contributor: Contributor,
    /// Signature from the inviter proving authorization
    pub signature: Signature,
}

impl SignedContributor {
    /// Create a new signed contributor invitation.
    pub fn new(contributor: Contributor, inviter_key: &SigningKey) -> Self {
        debug_assert_eq!(
            contributor.invited_by,
            ContributorId::from(inviter_key.verifying_key()),
            "invited_by must match the inviter's key"
        );
        Self {
            signature: sign_struct(&contributor, inviter_key),
            contributor,
        }
    }

    /// Get this contributor's ID.
    pub fn id(&self) -> ContributorId {
        self.contributor.id()
    }

    /// Verify the invitation signature.
    pub fn verify_signature(&self, inviter_vk: &VerifyingKey) -> Result<(), String> {
        verify_struct(&self.contributor, &self.signature, inviter_vk)
            .map_err(|e| format!("Invalid contributor signature: {}", e))
    }
}

/// Core contributor data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contributor {
    /// The wiki owner's ID (for context)
    pub wiki_owner_id: ContributorId,
    /// Who invited this contributor
    pub invited_by: ContributorId,
    /// This contributor's public key
    pub contributor_vk: VerifyingKey,
    /// When the invitation was created
    pub invited_at: DateTime<Utc>,
}

impl Contributor {
    /// Get this contributor's ID.
    pub fn id(&self) -> ContributorId {
        ContributorId::from(&self.contributor_vk)
    }
}

/// Collection of all contributors.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ContributorsV1 {
    pub contributors: Vec<SignedContributor>,
}

/// Delta for contributor updates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContributorsDelta {
    pub added: Vec<SignedContributor>,
}

impl ContributorsV1 {
    /// Check if a contributor ID is authorized (owner or invited).
    pub fn is_authorized(&self, id: &ContributorId, owner_id: &ContributorId) -> bool {
        if id == owner_id {
            return true;
        }
        self.contributors.iter().any(|c| &c.id() == id)
    }

    /// Get a contributor by ID.
    pub fn get(&self, id: &ContributorId) -> Option<&SignedContributor> {
        self.contributors.iter().find(|c| &c.id() == id)
    }

    /// Get the verifying key for a contributor.
    pub fn get_verifying_key(
        &self,
        id: &ContributorId,
        owner_vk: &VerifyingKey,
    ) -> Option<VerifyingKey> {
        let owner_id = ContributorId::from(owner_vk);
        if id == &owner_id {
            return Some(*owner_vk);
        }
        self.get(id).map(|c| c.contributor.contributor_vk)
    }

    /// Summarize as set of contributor IDs.
    pub fn summarize(&self) -> HashSet<ContributorId> {
        self.contributors.iter().map(|c| c.id()).collect()
    }

    /// Compute delta (new contributors not in old summary).
    pub fn delta(&self, old_summary: &HashSet<ContributorId>) -> Option<ContributorsDelta> {
        let added: Vec<_> = self
            .contributors
            .iter()
            .filter(|c| !old_summary.contains(&c.id()))
            .cloned()
            .collect();

        if added.is_empty() {
            None
        } else {
            Some(ContributorsDelta { added })
        }
    }

    /// Apply a delta, verifying each new contributor.
    pub fn apply_delta(
        &mut self,
        delta: &ContributorsDelta,
        owner_vk: &VerifyingKey,
        max_contributors: usize,
    ) -> Result<(), String> {
        let owner_id = ContributorId::from(owner_vk);

        for contributor in &delta.added {
            // Skip duplicates
            if self.contributors.iter().any(|c| c.id() == contributor.id()) {
                continue;
            }

            // Verify the invitation chain
            self.verify_invite(&contributor, owner_vk, &owner_id)?;

            // Enforce limit
            if self.contributors.len() >= max_contributors {
                return Err("Maximum contributors reached".to_string());
            }

            self.contributors.push(contributor.clone());
        }

        Ok(())
    }

    /// Verify an invitation is valid.
    fn verify_invite(
        &self,
        contributor: &SignedContributor,
        owner_vk: &VerifyingKey,
        owner_id: &ContributorId,
    ) -> Result<(), String> {
        let inviter_id = &contributor.contributor.invited_by;

        // Get the inviter's verifying key
        let inviter_vk = if inviter_id == owner_id {
            *owner_vk
        } else {
            self.get(inviter_id)
                .ok_or_else(|| format!("Inviter {:?} not found", inviter_id))?
                .contributor
                .contributor_vk
        };

        // Verify signature
        contributor.verify_signature(&inviter_vk)?;

        // Verify wiki owner matches
        if contributor.contributor.wiki_owner_id != *owner_id {
            return Err("Wiki owner ID mismatch".to_string());
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
    fn test_owner_is_always_authorized() {
        let owner_key = generate_key();
        let owner_id = ContributorId::from(owner_key.verifying_key());
        let contributors = ContributorsV1::default();

        assert!(contributors.is_authorized(&owner_id, &owner_id));
    }

    #[test]
    fn test_invite_contributor() {
        let owner_key = generate_key();
        let owner_vk = owner_key.verifying_key();
        let owner_id = ContributorId::from(&owner_vk);

        let contributor_key = generate_key();
        let contributor = Contributor {
            wiki_owner_id: owner_id,
            invited_by: owner_id,
            contributor_vk: contributor_key.verifying_key(),
            invited_at: Utc::now(),
        };
        let signed = SignedContributor::new(contributor, &owner_key);

        let mut contributors = ContributorsV1::default();
        let delta = ContributorsDelta {
            added: vec![signed.clone()],
        };

        contributors.apply_delta(&delta, &owner_vk, 100).unwrap();

        assert!(contributors.is_authorized(&signed.id(), &owner_id));
    }

    #[test]
    fn test_invalid_signature_rejected() {
        use crate::util::sign_struct;

        let owner_key = generate_key();
        let owner_vk = owner_key.verifying_key();
        let owner_id = ContributorId::from(&owner_vk);

        let attacker_key = generate_key();
        let contributor_key = generate_key();

        let contributor = Contributor {
            wiki_owner_id: owner_id,
            invited_by: owner_id,
            contributor_vk: contributor_key.verifying_key(),
            invited_at: Utc::now(),
        };
        // Manually construct with wrong signature (bypassing debug_assert)
        let signed = SignedContributor {
            signature: sign_struct(&contributor, &attacker_key),
            contributor,
        };

        let mut contributors = ContributorsV1::default();
        let delta = ContributorsDelta {
            added: vec![signed],
        };

        assert!(contributors.apply_delta(&delta, &owner_vk, 100).is_err());
    }
}
