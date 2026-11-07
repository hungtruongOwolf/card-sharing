//! Provider adapters (genai-effects "Pipeline"): every generation goes through
//! [`EffectProvider`], so a real provider (fal.ai, Vertex, ...) is one more adapter behind the same
//! contract. Only the fake exists today: it transforms pixels locally and never calls a network.

use std::future::Future;
use std::pin::Pin;

use image::{Rgba, RgbaImage};

use crate::settings::{FakeTransform, Template};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One generation request: a curated template applied to the user's (already clean) image.
pub struct EffectRequest<'a> {
    pub template: &'a Template,
    pub image: &'a RgbaImage,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// Outage, timeout, rate limit: retry later; refund when out of attempts.
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    /// The provider's own safety system refused the request: refund, do not retry.
    #[error("provider refused the request")]
    Refused,
}

pub trait EffectProvider: Send + Sync {
    /// Stored on the job (audit, C2PA claim generator).
    fn name(&self) -> &'static str;
    fn generate<'a>(
        &'a self,
        request: EffectRequest<'a>,
    ) -> BoxFuture<'a, Result<RgbaImage, ProviderError>>;
}

/// Deterministic local stand-in for a real provider (`API_AI_EFFECT_PROVIDER=fake`, refused in
/// production). Applies the template's `fake_transform`; alpha is kept, so cutouts stay cutouts.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeProvider;

impl EffectProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn generate<'a>(
        &'a self,
        request: EffectRequest<'a>,
    ) -> BoxFuture<'a, Result<RgbaImage, ProviderError>> {
        let transform = request.template.fake_transform;
        let mut out = request.image.clone();
        Box::pin(async move {
            for pixel in out.pixels_mut() {
                *pixel = apply(transform, *pixel);
            }
            Ok(out)
        })
    }
}

fn apply(transform: FakeTransform, Rgba([r, g, b, a]): Rgba<u8>) -> Rgba<u8> {
    match transform {
        FakeTransform::Invert => Rgba([255 - r, 255 - g, 255 - b, a]),
        FakeTransform::Grayscale => {
            let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
            let luma = u8::try_from(luma).unwrap_or(u8::MAX);
            Rgba([luma, luma, luma, a])
        }
        FakeTransform::Posterize => Rgba([r & 0xC0, g & 0xC0, b & 0xC0, a]),
    }
}

/// No provider configured (`API_AI_EFFECT_PROVIDER=none`): every request is unavailable, so a
/// job left over from a configured period retries and is refunded on its last attempt.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredProvider;

impl EffectProvider for UnconfiguredProvider {
    fn name(&self) -> &'static str {
        "none"
    }

    fn generate<'a>(
        &'a self,
        _request: EffectRequest<'a>,
    ) -> BoxFuture<'a, Result<RgbaImage, ProviderError>> {
        Box::pin(async {
            Err(ProviderError::Unavailable(
                "no provider configured".to_owned(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(transform: FakeTransform) -> Template {
        Template {
            id: "ai.test".to_owned(),
            version: 1,
            title: "Test".to_owned(),
            cost: 1,
            enabled: true,
            prompt: "unused".to_owned(),
            fake_transform: transform,
        }
    }

    #[tokio::test]
    async fn fake_transforms_pixels_and_keeps_alpha_and_size() {
        let image = RgbaImage::from_pixel(3, 2, Rgba([10, 200, 77, 128]));
        for (transform, expected) in [
            (FakeTransform::Invert, Rgba([245, 55, 178, 128])),
            (FakeTransform::Grayscale, Rgba([129, 129, 129, 128])),
            (FakeTransform::Posterize, Rgba([0, 192, 64, 128])),
        ] {
            let template = template(transform);
            let out = FakeProvider
                .generate(EffectRequest {
                    template: &template,
                    image: &image,
                })
                .await
                .unwrap();
            assert_eq!(out.dimensions(), (3, 2));
            assert!(out.pixels().all(|p| *p == expected), "{transform:?}");
        }
    }
}
