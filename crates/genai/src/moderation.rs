//! Moderation hooks around generation (genai-effects "moderation in + out"). The input gate runs
//! before the provider sees the photo, the output gate before the result is stored. Both are on
//! top of the media pipeline's own gates (known-CSAM hashes, nudity classifier), which every
//! output passes as a normal upload.

use image::RgbaImage;

use crate::provider::BoxFuture;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Refuse,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("moderation gate unavailable: {0}")]
pub struct GateError(pub String);

pub trait ImageGate: Send + Sync {
    fn check<'a>(&'a self, image: &'a RgbaImage) -> BoxFuture<'a, Result<Verdict, GateError>>;
}

/// The input and output gates of the pipeline.
pub struct Gates {
    pub input: Box<dyn ImageGate>,
    pub output: Box<dyn ImageGate>,
}

impl Gates {
    /// Deterministic fakes (tests, local development; never production): the input gate refuses
    /// an image whose top-left pixel is [`FAKE_INPUT_REFUSAL`], the output gate one whose
    /// top-left pixel is [`FAKE_OUTPUT_REFUSAL`]. The colours do not collide with the media
    /// pipeline's fake markers (magenta, cyan, yellow, blue).
    pub fn fake() -> Self {
        Self {
            input: Box::new(FakeGate(FAKE_INPUT_REFUSAL)),
            output: Box::new(FakeGate(FAKE_OUTPUT_REFUSAL)),
        }
    }
}

/// Top-left pixel the fake input gate refuses (pure green).
pub const FAKE_INPUT_REFUSAL: [u8; 3] = [0, 255, 0];
/// Top-left pixel the fake output gate refuses (orange). The fake `invert` template turns an
/// input pixel of `[0, 127, 255]` into it.
pub const FAKE_OUTPUT_REFUSAL: [u8; 3] = [255, 128, 0];

struct FakeGate([u8; 3]);

impl ImageGate for FakeGate {
    fn check<'a>(&'a self, image: &'a RgbaImage) -> BoxFuture<'a, Result<Verdict, GateError>> {
        let refused =
            image.width() > 0 && image.height() > 0 && image.get_pixel(0, 0).0[..3] == self.0;
        Box::pin(async move {
            Ok(if refused {
                Verdict::Refuse
            } else {
                Verdict::Pass
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[tokio::test]
    async fn fake_gates_refuse_only_their_marker() {
        let gates = Gates::fake();
        let [r, g, b] = FAKE_INPUT_REFUSAL;
        let marked = RgbaImage::from_pixel(2, 2, Rgba([r, g, b, 255]));
        let plain = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
        assert_eq!(gates.input.check(&marked).await, Ok(Verdict::Refuse));
        assert_eq!(gates.input.check(&plain).await, Ok(Verdict::Pass));
        assert_eq!(gates.output.check(&marked).await, Ok(Verdict::Pass));
        let [r, g, b] = FAKE_OUTPUT_REFUSAL;
        let out = RgbaImage::from_pixel(2, 2, Rgba([r, g, b, 255]));
        assert_eq!(gates.output.check(&out).await, Ok(Verdict::Refuse));
    }
}
