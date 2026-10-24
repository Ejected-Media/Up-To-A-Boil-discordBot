I'muse anyhow::{Context, Result};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use reqwest::Client;
use serde_json::{json, Value};

pub struct NotionClient {
    http: Client,
    api_key: String,
    parent_page_id: String,
}

impl NotionClient {
    pub fn new(api_key: String, parent_page_id: String) -> Self {
        Self {
            http: Client::new(),
            api_key,
            parent_page_id,
        }
    }

    /// Converts standard Markdown text into a Vec of Notion Block JSON objects
    pub fn markdown_to_blocks(md: &str) -> Vec<Value> {
        let parser = Parser::new(md);
        let mut blocks = Vec::new();
        let mut current_text = String::new();
        let mut current_block_type: Option<&str> = None;

        for event in parser {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current_text.clear();
                    current_block_type = Some(match level {
                        HeadingLevel::H1 => "heading_1",
                        HeadingLevel::H2 => "heading_2",
                        _ => "heading_3",
                    });
                }
                Event::Start(Tag::Paragraph) => {
                    current_text.clear();
                    current_block_type = Some("paragraph");
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    current_text.clear();
                    current_block_type = Some("code");
                }
                Event::Text(text) | Event::Code(text) => {
                    current_text.push_str(&text);
                }
                Event::End(TagEnd::Heading(_))
                | Event::End(TagEnd::Paragraph)
                | Event::End(TagEnd::CodeBlock) => {
                    if let Some(b_type) = current_block_type.take() {
                        let block = if b_type == "code" {
                            json!({
                                "object": "block",
                                "type": "code",
                                "code": {
                                    "rich_text": [{ "type": "text", "text": { "content": current_text } }],
                                    "language": "rust"
                                }
                            })
                        } else {
                            json!({
                                "object": "block",
                                "type": b_type,
                                b_type: {
                                    "rich_text": [{ "type": "text", "text": { "content": current_text } }]
                                }
                            })
                        };
                        blocks.push(block);
                    }
                }
                _ => {}
            }
        }
        blocks
    }

    /// Creates a new Notion page under the configured parent page
    pub async fn push_new_page(&self, title: &str, markdown: &str) -> Result<String> {
        let children = Self::markdown_to_blocks(markdown);
        let payload = json!({
            "parent": { "type": "page_id", "page_id": self.parent_page_id },
            "properties": {
                "title": {
                    "title": [{ "type": "text", "text": { "content": title } }]
                }
            },
            "children": children
        });

        let res = self
            .http
            .post("https://api.notion.com/v1/pages")
            .bearer_auth(&self.api_key)
            .header("Notion-Version", "2022-06-28")
            .json(&payload)
            .send()
            .await
            .context("Network error reaching Notion API")?;

        let status = res.status();
        let body: Value = res.json().await?;

        if !status.is_success() {
            anyhow::bail!("Notion API Error ({}): {}", status, body);
        }

        let page_id = body["id"]
            .as_str()
            .context("Missing page 'id' in Notion response")?
            .to_string();

        Ok(page_id)
    }
}

    /// Creates a new entry inside a Notion Database with mapped YAML properties
    pub async fn push_database_entry(
        &self,
        properties: Value,
        markdown_body: &str,
    ) -> Result<String> {
        let children = Self::markdown_to_blocks(markdown_body);
        let payload = json!({
            "parent": {
                "type": "database_id",
                "database_id": self.parent_page_id
            },
            "properties": properties,
            "children": children
        });

        let res = self
            .http
            .post("https://api.notion.com/v1/pages")
            .bearer_auth(&self.api_key)
            .header("Notion-Version", "2022-06-28")
            .json(&payload)
            .send()
            .await
            .context("Network error reaching Notion API")?;

        let status = res.status();
        let body: Value = res.json().await?;

        if !status.is_success() {
            anyhow::bail!("Notion API Error ({}): {}", status, body);
        }

        let page_id = body["id"]
            .as_str()
            .context("Missing page 'id' in Notion response")?
            .to_string();

        Ok(page_id)
    }

