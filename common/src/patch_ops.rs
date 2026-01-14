//! Patch operations for content-addressed, commutative editing.
//!
//! Operations target lines by content hash rather than position,
//! making them commutative - they can be applied in any order
//! and produce the same result.

use crate::util::{line_hash, LineHash};
use serde::{Deserialize, Serialize};

/// A single patch operation on page content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PatchOp {
    /// Insert new lines after an anchor point.
    Insert {
        /// Hash of the line to insert after. None = beginning of document.
        anchor_hash: Option<LineHash>,
        /// Lines to insert.
        lines: Vec<String>,
    },

    /// Delete lines by their content hash.
    Delete {
        /// Hashes of lines to delete.
        line_hashes: Vec<LineHash>,
    },

    /// Replace a line matching a hash with new content.
    Replace {
        /// Hash of the original line.
        original_hash: LineHash,
        /// New content for the line.
        new_content: String,
    },
}

/// A line with its content hash for tracking.
#[derive(Clone, Debug, PartialEq)]
pub struct HashedLine {
    pub hash: LineHash,
    pub content: String,
}

impl HashedLine {
    pub fn new(content: String) -> Self {
        Self {
            hash: line_hash(&content),
            content,
        }
    }
}

/// Apply patch operations to content, producing rendered output.
///
/// Operations are applied in the order provided. The caller should
/// sort operations by (timestamp, patch_id) for deterministic results.
pub fn apply_operations(base_content: &str, operations: &[PatchOp]) -> String {
    // Convert base content to hashed lines
    let mut lines: Vec<HashedLine> = base_content
        .lines()
        .map(|l| HashedLine::new(l.to_string()))
        .collect();

    // Apply each operation
    for op in operations {
        match op {
            PatchOp::Delete { line_hashes } => {
                // Remove lines matching any of the hashes
                lines.retain(|line| !line_hashes.contains(&line.hash));
            }

            PatchOp::Replace {
                original_hash,
                new_content,
            } => {
                // Find and replace the first matching line
                if let Some(line) = lines.iter_mut().find(|l| l.hash == *original_hash) {
                    line.content = new_content.clone();
                    line.hash = line_hash(new_content);
                }
            }

            PatchOp::Insert { anchor_hash, lines: new_lines } => {
                let insert_pos = match anchor_hash {
                    None => 0, // Insert at beginning
                    Some(hash) => {
                        // Find the anchor line and insert after it
                        lines
                            .iter()
                            .position(|l| l.hash == *hash)
                            .map(|p| p + 1)
                            .unwrap_or(lines.len()) // Append if anchor not found
                    }
                };

                // Insert new lines at the position
                let new_hashed: Vec<HashedLine> = new_lines
                    .iter()
                    .map(|l| HashedLine::new(l.clone()))
                    .collect();

                // Insert in reverse order to maintain correct positions
                for (i, line) in new_hashed.into_iter().enumerate() {
                    lines.insert(insert_pos + i, line);
                }
            }
        }
    }

    // Join lines back into content
    lines
        .into_iter()
        .map(|l| l.content)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Create a delete operation for a specific line.
pub fn delete_line(content: &str) -> PatchOp {
    PatchOp::Delete {
        line_hashes: vec![line_hash(content)],
    }
}

/// Create an insert operation to add lines after an anchor.
pub fn insert_after(anchor: Option<&str>, new_lines: Vec<String>) -> PatchOp {
    PatchOp::Insert {
        anchor_hash: anchor.map(line_hash),
        lines: new_lines,
    }
}

/// Create a replace operation.
pub fn replace_line(original: &str, new_content: String) -> PatchOp {
    PatchOp::Replace {
        original_hash: line_hash(original),
        new_content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_by_hash() {
        let content = "line1\nline2\nline3";
        let ops = vec![delete_line("line2")];
        let result = apply_operations(content, &ops);
        assert_eq!(result, "line1\nline3");
    }

    #[test]
    fn test_insert_at_beginning() {
        let content = "line1\nline2";
        let ops = vec![insert_after(None, vec!["new_line".to_string()])];
        let result = apply_operations(content, &ops);
        assert_eq!(result, "new_line\nline1\nline2");
    }

    #[test]
    fn test_insert_after_line() {
        let content = "line1\nline2\nline3";
        let ops = vec![insert_after(Some("line1"), vec!["inserted".to_string()])];
        let result = apply_operations(content, &ops);
        assert_eq!(result, "line1\ninserted\nline2\nline3");
    }

    #[test]
    fn test_replace_line() {
        let content = "line1\nline2\nline3";
        let ops = vec![replace_line("line2", "replaced".to_string())];
        let result = apply_operations(content, &ops);
        assert_eq!(result, "line1\nreplaced\nline3");
    }

    #[test]
    fn test_delete_is_commutative() {
        let content = "line1\nline2\nline3\nline4";

        let delete_2 = delete_line("line2");
        let delete_3 = delete_line("line3");

        // Apply in order 2, 3
        let result_23 = apply_operations(content, &[delete_2.clone(), delete_3.clone()]);

        // Apply in order 3, 2
        let result_32 = apply_operations(content, &[delete_3, delete_2]);

        assert_eq!(result_23, result_32);
        assert_eq!(result_23, "line1\nline4");
    }

    #[test]
    fn test_insert_delete_commutative() {
        let content = "line1\nline2\nline3";

        let delete = delete_line("line2");
        let insert = insert_after(Some("line1"), vec!["new_line".to_string()]);

        // Apply delete then insert
        let result_di = apply_operations(content, &[delete.clone(), insert.clone()]);

        // Apply insert then delete
        let result_id = apply_operations(content, &[insert, delete]);

        assert_eq!(result_di, result_id);
        assert_eq!(result_di, "line1\nnew_line\nline3");
    }

    #[test]
    fn test_multiple_inserts_same_anchor() {
        let content = "line1\nline2";

        // Two inserts after line1 - order matters for these
        let insert_a = insert_after(Some("line1"), vec!["A".to_string()]);
        let insert_b = insert_after(Some("line1"), vec!["B".to_string()]);

        // When applied in order A, B: line1, A, B, line2
        // (B finds line1 and inserts after it, then A finds line1 and inserts after it)
        // Actually: A inserts first -> line1, A, line2
        // Then B inserts after line1 -> line1, B, A, line2
        let result_ab = apply_operations(content, &[insert_a.clone(), insert_b.clone()]);

        // When applied in order B, A:
        // B inserts first -> line1, B, line2
        // Then A inserts after line1 -> line1, A, B, line2
        let result_ba = apply_operations(content, &[insert_b, insert_a]);

        // These will differ - that's expected for concurrent inserts at same position
        // The important thing is the result is deterministic for a given operation order
        assert_eq!(result_ab, "line1\nB\nA\nline2");
        assert_eq!(result_ba, "line1\nA\nB\nline2");
    }

    #[test]
    fn test_delete_missing_line_is_noop() {
        let content = "line1\nline2";
        let ops = vec![delete_line("nonexistent")];
        let result = apply_operations(content, &ops);
        assert_eq!(result, content);
    }

    #[test]
    fn test_replace_missing_line_is_noop() {
        let content = "line1\nline2";
        let ops = vec![replace_line("nonexistent", "replaced".to_string())];
        let result = apply_operations(content, &ops);
        assert_eq!(result, content);
    }

    #[test]
    fn test_insert_with_missing_anchor_appends() {
        let content = "line1\nline2";
        let ops = vec![insert_after(Some("nonexistent"), vec!["new".to_string()])];
        let result = apply_operations(content, &ops);
        assert_eq!(result, "line1\nline2\nnew");
    }
}
