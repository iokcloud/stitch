//! Agent session management.
//!
//! Tracks the state of a single conversation: message history,
//! token usage, and confirmation state. The message format
//! matches the OpenAI Chat Completions API shape.

use serde::{Deserialize, Serialize};

/// A single message in the conversation.
///
/// Serializes to the OpenAI chat message format. `content` is a plain string
/// for text-only messages (the wire format is byte-identical to the old
/// `String` field); image-carrying user messages use the OpenAI content
/// array shape (`[{type, ...}, ...]`) via [`Content::Parts`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Content,
    /// Tool calls made by the assistant (only present for assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// ID of the tool call this message is responding to (only present for tool messages).
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    pub tool_call_id: Option<String>,
}

/// Message content: either a plain string (OpenAI `"content": "..."`) or an
/// array of typed parts (OpenAI `"content": [{"type": ...}, ...]`) for
/// multimodal messages. `untagged` keeps the old wire format and makes
/// existing `messages.jsonl` deserialize as `Text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// One element of a multimodal content array (OpenAI format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

/// `image_url` payload of an image part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

impl Content {
    /// The text of this message: `Text` as-is; `Parts` takes the first text
    /// part (constructed messages always have at most one).
    pub fn text(&self) -> &str {
        match self {
            Content::Text(s) => s,
            Content::Parts(parts) => parts
                .iter()
                .find_map(|p| match p {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .unwrap_or(""),
        }
    }

    /// Mutable text: `Parts` without a text part gets an empty one first.
    pub fn text_mut(&mut self) -> &mut String {
        match self {
            Content::Text(s) => s,
            Content::Parts(parts) => {
                if !parts.iter().any(|p| matches!(p, ContentPart::Text { .. })) {
                    parts.insert(
                        0,
                        ContentPart::Text {
                            text: String::new(),
                        },
                    );
                }
                let mut idx = 0;
                for (i, p) in parts.iter().enumerate() {
                    if matches!(p, ContentPart::Text { .. }) {
                        idx = i;
                        break;
                    }
                }
                match &mut parts[idx] {
                    ContentPart::Text { text } => text,
                    _ => unreachable!(),
                }
            }
        }
    }

    pub fn text_is_empty(&self) -> bool {
        self.text().is_empty()
    }

    pub fn image_count(&self) -> usize {
        match self {
            Content::Text(_) => 0,
            Content::Parts(parts) => parts
                .iter()
                .filter(|p| matches!(p, ContentPart::ImageUrl { .. }))
                .count(),
        }
    }

    /// Turn into a part list (plain text becomes a single text part).
    pub fn into_parts(self) -> Vec<ContentPart> {
        match self {
            Content::Text(s) => vec![ContentPart::Text { text: s }],
            Content::Parts(parts) => parts,
        }
    }

    /// Prepend a text block; image parts are preserved and the block becomes
    /// the leading text part (used by archived-context promotion).
    pub fn prepend_text(&mut self, prefix: &str) {
        match self {
            Content::Text(s) => *s = format!("{prefix}{s}"),
            Content::Parts(parts) => {
                let merged = if let Some(ContentPart::Text { text }) = parts.first_mut() {
                    let new_text = format!("{prefix}{text}");
                    *text = new_text;
                    return;
                } else {
                    format!("{prefix}")
                };
                parts.insert(0, ContentPart::Text { text: merged });
            }
        }
    }

    pub fn contains(&self, pat: &str) -> bool {
        self.text().contains(pat)
    }

    /// Lossy text for write-back paths that only ever carry plain text
    /// (tool-message externalization); images are dropped.
    pub fn into_string_lossy(self) -> String {
        self.text().to_string()
    }

    /// Remove image parts for the on-disk copy (lightweight backend: image
    /// data URLs never persist). Text survives as-is; an image-only message
    /// becomes the `[图片]` stub, so the disk stays plain-text wire format.
    /// Only the disk copy is stripped — the in-memory session keeps the
    /// images for the current turn; after a restart the model no longer
    /// sees archived images (accepted trade-off).
    pub fn strip_images(&mut self) {
        match self {
            Content::Text(_) => {}
            Content::Parts(parts) => {
                parts.retain(|p| !matches!(p, ContentPart::ImageUrl { .. }));
                match parts.len() {
                    0 => *self = Content::Text(IMAGE_STRIPPED_STUB.to_string()),
                    1 => {
                        // Single text part collapses back to the plain-string
                        // wire format, keeping legacy jsonl readers unchanged.
                        if let ContentPart::Text { text } = &parts[0] {
                            *self = Content::Text(text.clone());
                        }
                    }
                    _ => {} // defensive: constructors never produce >1 text part
                }
            }
        }
    }
}

/// Placeholder left in the on-disk copy when image parts are stripped.
pub const IMAGE_STRIPPED_STUB: &str = "[图片]";

impl From<String> for Content {
    fn from(s: String) -> Self {
        Content::Text(s)
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Content::Text(s.to_string())
    }
}

impl From<&String> for Content {
    fn from(s: &String) -> Self {
        Content::Text(s.clone())
    }
}

/// Build a user message content from text plus image data URLs. Empty text
/// omits the text part entirely (some providers reject empty text parts);
/// URLs not starting with `data:image/` are dropped with a warning.
pub fn user_content_with_images(text: &str, urls: &[String]) -> Content {
    let mut parts = Vec::with_capacity(urls.len() + 1);
    if !text.is_empty() {
        parts.push(ContentPart::Text {
            text: text.to_string(),
        });
    }
    for url in urls {
        if url.starts_with("data:image/") {
            parts.push(ContentPart::ImageUrl {
                image_url: ImageUrl { url: url.clone() },
            });
        } else {
            tracing::warn!(len = url.len(), "dropping non-data image url");
        }
    }
    if parts.is_empty() {
        Content::Text(text.to_string())
    } else {
        Content::Parts(parts)
    }
}

/// A tool call requested by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// Always "function" for OpenAI.
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// The function name and arguments for a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

/// Message role matching OpenAI's API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    /// Result of a tool execution.
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// The session tracks one conversation with the agent.
#[derive(Debug, Clone)]
pub struct Session {
    /// All messages in this conversation, including system prompt.
    pub messages: Vec<Message>,
    /// Number of ReAct loop iterations executed so far.
    pub iteration: usize,
    /// Total tokens consumed (approximate).
    pub tokens_used: usize,
    /// Committed compaction epoch (ADR-036). Bumped on hard compact.
    pub epoch: u32,
    /// Three-tier context layering state (hot/warm/cold). None on CLI paths.
    pub layers: Option<crate::agent::layers::LayerManager>,
}

impl Session {
    pub fn new(system_prompt: impl Into<Content>) -> Self {
        Self {
            messages: vec![Message {
                role: Role::System,
                content: system_prompt.into(),
                tool_calls: None,
                tool_call_id: None,
            }],
            iteration: 0,
            tokens_used: 0,
            epoch: 0,
            layers: Some(crate::agent::layers::LayerManager::default()),
        }
    }

    pub fn add_user_message(&mut self, content: impl Into<Content>) {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    pub fn add_assistant_message(&mut self, content: impl Into<Content>) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    /// Add an assistant message with tool calls.
    pub fn add_assistant_tool_calls(
        &mut self,
        content: impl Into<Content>,
        tool_calls: Vec<ToolCall>,
    ) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        });
    }

    /// Add a tool result message.
    pub fn add_tool_result(&mut self, tool_call_id: String, content: impl Into<Content>) {
        self.messages.push(Message {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_text_serializes_as_plain_string() {
        let msg = Message {
            role: Role::User,
            content: "hi".into(),
            tool_calls: None,
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""content":"hi""#));
    }

    #[test]
    fn content_parts_serialize_as_openai_array() {
        let msg = Message {
            role: Role::User,
            content: Content::Parts(vec![
                ContentPart::Text {
                    text: "看这个".into(),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: "data:image/png;base64,AAAA".into(),
                    },
                },
            ]),
            tool_calls: None,
            tool_call_id: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"text","text":"看这个""#));
        assert!(
            json.contains(r#""type":"image_url","image_url":{"url":"data:image/png;base64,AAAA""#)
        );
    }

    #[test]
    fn legacy_jsonl_line_deserializes_as_text() {
        let msg: Message =
            serde_json::from_str(r#"{"role":"user","content":"legacy line"}"#).unwrap();
        assert!(matches!(msg.content, Content::Text(_)));
        assert_eq!(msg.content.text(), "legacy line");
    }

    #[test]
    fn user_content_with_images_omits_empty_text_part() {
        let c = user_content_with_images("", &["data:image/png;base64,BB".into()]);
        let Content::Parts(parts) = &c else {
            panic!("expected parts");
        };
        assert_eq!(parts.len(), 1);
        assert!(matches!(parts[0], ContentPart::ImageUrl { .. }));

        // Invalid urls are dropped, text part survives.
        let c2 = user_content_with_images("keep", &["not-data:foo".into()]);
        assert_eq!(c2.text(), "keep");
        assert_eq!(c2.image_count(), 0);

        // Empty text + all-invalid urls falls back to plain empty text.
        let c3 = user_content_with_images("", &["junk".into()]);
        assert!(matches!(c3, Content::Text(_)));
        assert!(c3.text().is_empty());
    }

    #[test]
    fn prepend_text_preserves_image_parts() {
        let mut c = user_content_with_images("原文本", &["data:image/png;base64,CC".into()]);
        c.prepend_text("[归档恢复] 摘要\n\n");
        assert_eq!(c.image_count(), 1);
        assert!(c.text().starts_with("[归档恢复]"));
        assert!(c.text().ends_with("原文本"));
    }

    #[test]
    fn text_mut_adds_text_part_to_pure_image_content() {
        let mut c = user_content_with_images("", &["data:image/png;base64,DD".into()]);
        assert_eq!(c.text(), "");
        *c.text_mut() = "补上说明".into();
        assert_eq!(c.text(), "补上说明");
        assert_eq!(c.image_count(), 1);
    }

    #[test]
    fn strip_images_text_untouched() {
        let mut c: Content = "纯文本消息".into();
        c.strip_images();
        assert!(matches!(c, Content::Text(_)));
        assert_eq!(c.text(), "纯文本消息");
    }

    #[test]
    fn strip_images_text_plus_image_collapses() {
        let mut c = user_content_with_images("看这张图", &["data:image/png;base64,EE".into()]);
        assert_eq!(c.image_count(), 1);
        c.strip_images();
        assert!(matches!(c, Content::Text(_))); // back to plain-string wire format
        assert_eq!(c.text(), "看这张图");
        assert_eq!(c.image_count(), 0);
    }

    #[test]
    fn strip_images_image_only_gets_stub() {
        let mut c = user_content_with_images("", &["data:image/png;base64,FF".into()]);
        c.strip_images();
        assert!(matches!(c, Content::Text(_)));
        assert_eq!(c.text(), IMAGE_STRIPPED_STUB);
    }

    #[test]
    fn strip_images_idempotent() {
        let mut c = user_content_with_images("文字加图", &["data:image/png;base64,GG".into()]);
        c.strip_images();
        let once = c.clone();
        c.strip_images();
        assert_eq!(c, once);
    }
}
