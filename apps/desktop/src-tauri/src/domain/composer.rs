use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CoreError, MediaMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectPreset {
    Source,
    Landscape16x9,
    Square1x1,
    Vertical9x16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewPreset {
    Draft,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleMode {
    None,
    Soft,
    Burned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlipMode {
    None,
    Horizontal,
    Vertical,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageOverlayKind {
    Logo,
    Watermark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextOverlay {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub font_size: u32,
    pub color: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOverlay {
    pub artifact_id: Uuid,
    pub kind: ImageOverlayKind,
    pub region: TimedRegion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverRegion {
    pub region: TimedRegion,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlurRegion {
    pub region: TimedRegion,
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerConfig {
    pub project_id: Uuid,
    pub trim_start_ms: u64,
    pub trim_end_ms: Option<u64>,
    pub crop: Option<CropRect>,
    pub aspect: AspectPreset,
    pub padding_color: String,
    pub blur_radius: f32,
    pub flip: FlipMode,
    pub speed: f32,
    pub subtitle_mode: SubtitleMode,
    pub preview_preset: PreviewPreset,
    pub text_overlays: Vec<TextOverlay>,
    pub image_overlays: Vec<ImageOverlay>,
    pub cover_regions: Vec<CoverRegion>,
    #[serde(default)]
    pub blur_regions: Vec<BlurRegion>,
}

impl ComposerConfig {
    pub fn defaults(project_id: Uuid) -> Self {
        Self {
            project_id,
            trim_start_ms: 0,
            trim_end_ms: None,
            crop: None,
            aspect: AspectPreset::Source,
            padding_color: "#000000".into(),
            blur_radius: 0.0,
            flip: FlipMode::None,
            speed: 1.0,
            subtitle_mode: SubtitleMode::Soft,
            preview_preset: PreviewPreset::Draft,
            text_overlays: Vec::new(),
            image_overlays: Vec::new(),
            cover_regions: Vec::new(),
            blur_regions: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self
            .trim_end_ms
            .is_some_and(|end| end <= self.trim_start_ms)
            || !valid_color(&self.padding_color)
            || !self.blur_radius.is_finite()
            || !(0.0..=20.0).contains(&self.blur_radius)
            || !self.speed.is_finite()
            || !(0.25..=4.0).contains(&self.speed)
            || self.text_overlays.len() > 8
            || self.image_overlays.len() > 4
            || self.cover_regions.len() > 8
            || self.blur_regions.len() > 8
        {
            return Err(CoreError::InvalidInput("composer config"));
        }
        for overlay in &self.text_overlays {
            if overlay.text.trim().is_empty()
                || overlay.text.chars().count() > 500
                || overlay.text.chars().any(|value| value.is_control())
                || !(8..=256).contains(&overlay.font_size)
                || !valid_color(&overlay.color)
                || overlay.end_ms <= overlay.start_ms
            {
                return Err(CoreError::InvalidInput("text overlay"));
            }
        }
        for region in self
            .image_overlays
            .iter()
            .map(|value| &value.region)
            .chain(self.cover_regions.iter().map(|value| &value.region))
            .chain(self.blur_regions.iter().map(|value| &value.region))
        {
            if !valid_region(region) {
                return Err(CoreError::InvalidInput("composer region"));
            }
        }
        if self
            .cover_regions
            .iter()
            .any(|value| !valid_color(&value.color))
        {
            return Err(CoreError::InvalidInput("cover color"));
        }
        if self
            .blur_regions
            .iter()
            .any(|value| !value.radius.is_finite() || !(1.0..=20.0).contains(&value.radius))
        {
            return Err(CoreError::InvalidInput("blur region"));
        }
        Ok(())
    }

    pub fn validate_for_source(&self, source: &MediaMetadata) -> Result<(), CoreError> {
        self.validate()?;
        let end = self.trim_end_ms.unwrap_or(source.duration_ms);
        if self.trim_start_ms >= source.duration_ms || end > source.duration_ms {
            return Err(CoreError::InvalidInput("composer trim"));
        }
        if self
            .text_overlays
            .iter()
            .map(|value| (value.start_ms, value.end_ms))
            .chain(
                self.image_overlays
                    .iter()
                    .map(|value| (value.region.start_ms, value.region.end_ms)),
            )
            .chain(
                self.cover_regions
                    .iter()
                    .map(|value| (value.region.start_ms, value.region.end_ms)),
            )
            .chain(
                self.blur_regions
                    .iter()
                    .map(|value| (value.region.start_ms, value.region.end_ms)),
            )
            .any(|(start, finish)| start >= source.duration_ms || finish > source.duration_ms)
        {
            return Err(CoreError::InvalidInput("composer timing bounds"));
        }
        if let Some(crop) = &self.crop {
            if crop.width < 2
                || crop.height < 2
                || crop.x.saturating_add(crop.width) > source.width
                || crop.y.saturating_add(crop.height) > source.height
            {
                return Err(CoreError::InvalidInput("composer crop"));
            }
        }
        let (width, height) = self.output_dimensions(source);
        if width < 2 || height < 2 {
            return Err(CoreError::InvalidInput("composer output dimensions"));
        }
        for (x, y, region_width, region_height) in self
            .text_overlays
            .iter()
            .map(|value| (value.x, value.y, 1, 1))
            .chain(self.image_overlays.iter().map(|value| {
                (
                    value.region.x,
                    value.region.y,
                    value.region.width,
                    value.region.height,
                )
            }))
            .chain(self.cover_regions.iter().map(|value| {
                (
                    value.region.x,
                    value.region.y,
                    value.region.width,
                    value.region.height,
                )
            }))
            .chain(self.blur_regions.iter().map(|value| {
                (
                    value.region.x,
                    value.region.y,
                    value.region.width,
                    value.region.height,
                )
            }))
        {
            if x >= width
                || y >= height
                || x.saturating_add(region_width) > width
                || y.saturating_add(region_height) > height
            {
                return Err(CoreError::InvalidInput("composer output bounds"));
            }
        }
        Ok(())
    }

    pub fn output_dimensions(&self, source: &MediaMetadata) -> (u32, u32) {
        match self.aspect {
            AspectPreset::Landscape16x9 => (1920, 1080),
            AspectPreset::Square1x1 => (1080, 1080),
            AspectPreset::Vertical9x16 => (1080, 1920),
            AspectPreset::Source => {
                let (width, height) = self
                    .crop
                    .as_ref()
                    .map_or((source.width, source.height), |crop| {
                        (crop.width, crop.height)
                    });
                (width - width % 2, height - height % 2)
            }
        }
    }

    pub fn expected_duration_ms(&self, source_duration_ms: u64) -> u64 {
        let end = self.trim_end_ms.unwrap_or(source_duration_ms);
        ((end - self.trim_start_ms) as f64 / self.speed as f64).round() as u64
    }
}

fn valid_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_region(region: &TimedRegion) -> bool {
    region.width > 0
        && region.height > 0
        && region.end_ms > region.start_ms
        && region.opacity.is_finite()
        && (0.0..=1.0).contains(&region.opacity)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderQualityReport {
    pub duration_ms: u64,
    pub expected_duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
    pub subtitle_mode: SubtitleMode,
    pub plan_hash: String,
}

impl RenderQualityReport {
    pub fn passes(&self) -> bool {
        self.has_video
            && self.has_audio
            && self.duration_ms > 0
            && self.expected_duration_ms > 0
            && self.width > 0
            && self.height > 0
            && self.duration_ms.abs_diff(self.expected_duration_ms) <= 250
            && self.plan_hash.len() == 64
            && self
                .plan_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }
}
