//! Per-turn preparation of a model call's history: rehydrating and inlining
//! image attachments for a vision model, or pointing a text-only model at the
//! `analyze_image` hand-off. Split out of `multimodal.rs` to keep it under the
//! layout limit; it only composes that module's marker helpers.

use tinyagents_session::transcript::{TranscriptMessage, TranscriptPart};

use super::markers;

/// Provider-only copy of a turn's history for a model call: on a vision-capable
/// model, rehydrate `[Image: … #att:<id>]` placeholders and then resolve every
/// marker into a provider-ready payload, as the channel harness does
/// (`agent::harness::graph`).
///
/// Rehydration alone leaves an `[IMAGE:<disk path>]` marker, which the message
/// bridge keeps as plain text, so the model receives a file path instead of the
/// image and describes something it never saw.
///
/// A non-vision model instead gets each placeholder rewritten into a note that
/// names `analyze_image`, the hand-off that can see it. Left as a placeholder,
/// the harness turns it into an image block and then, for a text-only target,
/// into "[image omitted]", and the model answers that it cannot see images.
/// The hand-off itself forwards the turn's attachment ids
/// (`attachment_placeholders`), not this text.
///
/// Callers must not persist the returned copy: it carries inline image bytes.
/// When preparation fails the rehydrated messages are returned and the error
/// is logged, matching the channel harness's fallback.
pub async fn prepare_turn_messages(
    messages: Vec<TranscriptMessage>,
    vision: bool,
    config: Option<&crate::config::Config>,
) -> Vec<TranscriptMessage> {
    if !super::has_image_placeholders(&messages) {
        return messages;
    }
    if !vision {
        return describe_placeholders_for_text_model(messages);
    }
    let rehydrated = super::rehydrate_image_placeholders(&messages);
    let defaults;
    let (images, files) = match config {
        Some(config) => (&config.multimodal, &config.multimodal_files),
        None => {
            defaults = (
                crate::config::MultimodalConfig::default(),
                crate::config::MultimodalFileConfig::default(),
            );
            (&defaults.0, &defaults.1)
        }
    };
    match super::prepare_messages_for_provider(&rehydrated, images, files).await {
        Ok(prepared) => {
            log::debug!(
                "[multimodal] prepared turn messages contains_images={} contains_files={}",
                prepared.contains_images,
                prepared.contains_files
            );
            prepared.messages
        }
        Err(error) => {
            log::warn!(
                "[multimodal] attachment preparation failed; sending markers as-is: {error}"
            );
            rehydrated
        }
    }
}

/// Rewrite every `[Image: … #att:<id>]` placeholder in `messages` into guidance
/// a text-only model can act on. The note avoids both the placeholder prefix
/// and the `#att:` reference so nothing downstream lifts it back into an image.
fn describe_placeholders_for_text_model(
    messages: Vec<TranscriptMessage>,
) -> Vec<TranscriptMessage> {
    fn rewrite(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(markers::IMAGE_PLACEHOLDER_PREFIX) {
            let Some(len) = rest[start..].find(']') else {
                break;
            };
            let token = &rest[start..start + len + 1];
            out.push_str(&rest[..start]);
            match token.find(markers::IMAGE_STASH_REF) {
                Some(at) => {
                    let id = token[at + markers::IMAGE_STASH_REF.len()..]
                        .trim_end_matches(']')
                        .trim();
                    out.push_str(&format!(
                        "[Attached image {id}: you cannot see it directly. Call the `analyze_image` tool with a `prompt` to have it described.]"
                    ));
                }
                None => out.push_str(token),
            }
            rest = &rest[start + len + 1..];
        }
        out.push_str(rest);
        out
    }
    let mut rewritten = 0usize;
    let messages: Vec<TranscriptMessage> = messages
        .into_iter()
        .map(|mut m| {
            if !super::row_has_image_placeholders(&m) {
                return m;
            }
            rewritten += 1;
            m.content = rewrite(&m.content);
            if let Some(parts) = m.parts.as_mut() {
                for part in parts.iter_mut() {
                    if let TranscriptPart::Text { text } = part {
                        *text = rewrite(text);
                    }
                }
            }
            m
        })
        .collect();
    log::debug!("[multimodal] text-only model: described image placeholders rows={rewritten}");
    messages
}
