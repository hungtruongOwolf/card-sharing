//! C2PA marking (EU AI Act Art. 50, genai-effects "Safety & compliance"): **stub**. It builds the
//! manifest a real signer will embed (claim generator, `c2pa.created` action with the IPTC
//! "trained algorithmic media" source type, the output hash) and stores it on the job, so the data
//! is there when `c2pa-rs` signing lands. Nothing is signed or embedded yet (`signed: false`).

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// IPTC digital source type for fully AI-generated media.
pub const TRAINED_ALGORITHMIC_MEDIA: &str =
    "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia";

/// The manifest for one generated image.
pub fn manifest(provider: &str, template_id: &str, template_version: u32, output: &[u8]) -> Value {
    let hash = Sha256::digest(output);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    json!({
        "claim_generator": format!("card-sharing/genai ({provider})"),
        "assertions": [{
            "label": "c2pa.actions",
            "data": { "actions": [{
                "action": "c2pa.created",
                "digitalSourceType": TRAINED_ALGORITHMIC_MEDIA,
                "softwareAgent": format!("{template_id}@{template_version}"),
            }] },
        }],
        "output_sha256": hex,
        "signed": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_marks_ai_generated_media_and_binds_the_bytes() {
        let m = manifest("fake", "ai.watercolor", 1, b"png");
        assert_eq!(
            m["assertions"][0]["data"]["actions"][0]["digitalSourceType"],
            TRAINED_ALGORITHMIC_MEDIA
        );
        assert_eq!(m["signed"], false);
        assert_ne!(
            m["output_sha256"],
            manifest("fake", "ai.watercolor", 1, b"other")["output_sha256"]
        );
    }
}
