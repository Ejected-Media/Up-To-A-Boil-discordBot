use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub date: Option<String>,
    pub published: Option<bool>,

    /// Captures arbitrary extra keys in your YAML frontmatter
    /// and maps them dynamically to Notion properties.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yml::Value>,
}

/// Splits a raw Markdown document into `(Option<Frontmatter>, &str)`.
/// Only triggers if the file starts cleanly with `---\n` or `---\r\n`.
pub fn split_frontmatter(raw: &str) -> Result<(Option<Frontmatter>, &str)> {
    let trimmed = raw.trim_start_matches('\u{feff}'); // Strip UTF-8 BOM if present

    let after_open = if let Some(rest) = trimmed.strip_prefix("---\n") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("---\r\n") {
        rest
    } else {
        return Ok((None, raw));
    };

    // Locate closing `---` delimiter on its own line
    if let Some(end_idx) = after_open.find("\n---") {
        let yaml_slice = &after_open[..end_idx];
        let remainder = &after_open[end_idx + 4..];

        // Strip the newline immediately following the closing `---`
        let markdown_body = remainder
            .strip_prefix("\r\n")
            .or_else(|| remainder.strip_prefix('\n'))
            .unwrap_or(remainder);

        let parsed: Frontmatter = serde_yml::from_str(yaml_slice)
            .context("Failed to deserialize YAML frontmatter")?;

        return Ok((Some(parsed), markdown_body));
    }

    Ok((None, raw))
}

impl Frontmatter {
    /// Converts parsed frontmatter into a Notion Database `properties` JSON object.
    /// `title_column` specifies the name of the primary title property in your Notion DB (usually "Name" or "Title").
    pub fn to_notion_properties(&self, fallback_title: &str, title_column: &str) -> Value {
        let mut props = Map::new();

        // 1. Primary Title Property (Required by Notion Databases)
        let resolved_title = self.title.as_deref().unwrap_or(fallback_title);
        props.insert(
            title_column.to_string(),
            json!({
                "title": [{ "type": "text", "text": { "content": resolved_title } }]
            }),
        );

        // 2. Status / Select Property
        if let Some(status) = &self.status {
            props.insert(
                "Status".to_string(),
                json!({
                    "select": { "name": status }
                }),
            );
        }

        // 3. Multi-Select Tags
        if !self.tags.is_empty() {
            let tag_objs: Vec<Value> = self
                .tags
                .iter()
                .map(|t| json!({ "name": t }))
                .collect();
            props.insert(
                "Tags".to_string(),
                json!({ "multi_select": tag_objs }),
            );
        }

        // 4. ISO-8601 Date Property (e.g., "2026-10-24")
        if let Some(date_str) = &self.date {
            props.insert(
                "Date".to_string(),
                json!({
                    "date": { "start": date_str }
                }),
            );
        }

        // 5. Checkbox Property
        if let Some(published) = self.published {
            props.insert(
                "Published".to_string(),
                json!({ "checkbox": published }),
            );
        }

        // 6. Dynamic Extra Keys (Numbers, Booleans, URLs, or Rich Text)
        for (key, val) in &self.extra {
            let mapped = match val {
                serde_yml::Value::Bool(b) => json!({ "checkbox": b }),
                serde_yml::Value::Number(n) => {
                    if let Some(f) = n.as_f64() {
                        json!({ "number": f })
                    } else {
                        continue;
                    }
                }
                serde_yml::Value::String(s) => {
                    if s.starts_with("http://") || s.starts_with("https://") {
                        json!({ "url": s })
                    } else {
                        json!({
                            "rich_text": [{ "type": "text", "text": { "content": s } }]
                        })
                    }
                }
                serde_yml::Value::Sequence(seq) => {
                    let items: Vec<Value> = seq
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| json!({ "name": s })))
                        .collect();
                    json!({ "multi_select": items })
                }
                _ => continue,
            };
            props.insert(key.clone(), mapped);
        }

        Value::Object(props)
    }
}
