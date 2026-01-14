//! Wiki link parsing and rendering.
//!
//! Supports [[Page Name]] and [[path|Display Text]] syntax.

use crate::page::PagePath;

/// A wiki link found in content.
#[derive(Clone, Debug, PartialEq)]
pub struct WikiLink {
    /// The target page path
    pub path: PagePath,
    /// Optional display text (if different from path)
    pub display: Option<String>,
    /// Start position in the original text
    pub start: usize,
    /// End position in the original text
    pub end: usize,
}

/// Extract wiki links from markdown content.
///
/// Supports two formats:
/// - `[[Page Name]]` - links to "page-name", displays "Page Name"
/// - `[[path/to/page|Display Text]]` - links to "path/to/page", displays "Display Text"
pub fn extract_wiki_links(content: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let mut chars = content.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        // Look for [[
        if c == '[' {
            if let Some((_, '[')) = chars.peek() {
                chars.next(); // consume second [
                let start = i;

                // Find the closing ]]
                let mut link_content = String::new();
                let mut end = start + 2;

                while let Some((j, c)) = chars.next() {
                    if c == ']' {
                        if let Some((_, ']')) = chars.peek() {
                            chars.next(); // consume second ]
                            end = j + 2;
                            break;
                        }
                    }
                    link_content.push(c);
                    end = j + 1;
                }

                if !link_content.is_empty() {
                    // Parse the link content
                    let (path_str, display) = if let Some(pipe_pos) = link_content.find('|') {
                        let path = &link_content[..pipe_pos];
                        let disp = &link_content[pipe_pos + 1..];
                        (path.to_string(), Some(disp.to_string()))
                    } else {
                        (link_content.clone(), None)
                    };

                    links.push(WikiLink {
                        path: PagePath::normalize(&path_str),
                        display,
                        start,
                        end,
                    });
                }
            }
        }
    }

    links
}

/// Render wiki links to HTML.
///
/// Converts `[[Page Name]]` to `<a href="#/page-name">Page Name</a>`
pub fn render_wiki_links(content: &str) -> String {
    let links = extract_wiki_links(content);
    if links.is_empty() {
        return content.to_string();
    }

    let mut result = String::with_capacity(content.len() * 2);
    let mut last_end = 0;

    for link in links {
        // Add content before this link
        result.push_str(&content[last_end..link.start]);

        // Render the link
        let display = link.display.as_ref().map(|s| s.as_str()).unwrap_or_else(|| {
            // Use the original text between [[ and ]] if no display text
            // (preserving original capitalization)
            &content[link.start + 2..link.end - 2].split('|').next().unwrap_or("")
        });

        result.push_str(&format!(
            "<a href=\"#/{}\">{}</a>",
            link.path.as_str(),
            html_escape(display)
        ));

        last_end = link.end;
    }

    // Add remaining content
    result.push_str(&content[last_end..]);

    result
}

/// Basic HTML escaping for display text.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Get all unique page paths linked from content.
pub fn get_linked_pages(content: &str) -> Vec<PagePath> {
    let links = extract_wiki_links(content);
    let mut paths: Vec<PagePath> = links.into_iter().map(|l| l.path).collect();
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_link() {
        let content = "See [[Home Page]] for more.";
        let links = extract_wiki_links(content);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].path.as_str(), "home page");
        assert_eq!(links[0].display, None);
    }

    #[test]
    fn test_link_with_display_text() {
        let content = "Check out the [[docs/api|API Documentation]].";
        let links = extract_wiki_links(content);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].path.as_str(), "docs/api");
        assert_eq!(links[0].display, Some("API Documentation".to_string()));
    }

    #[test]
    fn test_multiple_links() {
        let content = "See [[Home]] and [[About]] pages.";
        let links = extract_wiki_links(content);

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].path.as_str(), "home");
        assert_eq!(links[1].path.as_str(), "about");
    }

    #[test]
    fn test_render_simple_link() {
        let content = "See [[Home Page]] for more.";
        let rendered = render_wiki_links(content);

        assert_eq!(
            rendered,
            "See <a href=\"#/home page\">Home Page</a> for more."
        );
    }

    #[test]
    fn test_render_link_with_display() {
        let content = "Check [[docs/api|the API]].";
        let rendered = render_wiki_links(content);

        assert_eq!(
            rendered,
            "Check <a href=\"#/docs/api\">the API</a>."
        );
    }

    #[test]
    fn test_html_escape_in_display() {
        let content = "See [[test|<script>alert('xss')</script>]].";
        let rendered = render_wiki_links(content);

        assert!(rendered.contains("&lt;script&gt;"));
        assert!(!rendered.contains("<script>"));
    }

    #[test]
    fn test_no_links() {
        let content = "No links here.";
        let links = extract_wiki_links(content);
        assert!(links.is_empty());

        let rendered = render_wiki_links(content);
        assert_eq!(rendered, content);
    }

    #[test]
    fn test_get_linked_pages() {
        let content = "Links to [[Home]], [[About]], and [[Home]] again.";
        let pages = get_linked_pages(content);

        assert_eq!(pages.len(), 2); // Deduplicated
        assert!(pages.iter().any(|p| p.as_str() == "home"));
        assert!(pages.iter().any(|p| p.as_str() == "about"));
    }
}
