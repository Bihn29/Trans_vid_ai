use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSite {
    Douyin,
    Bilibili,
    YouTube,
    TikTok,
}

impl MediaSite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Douyin => "douyin",
            Self::Bilibili => "bilibili",
            Self::YouTube => "youtube",
            Self::TikTok => "tiktok",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub container: String,
    pub rotation_degrees: i32,
}

impl MediaMetadata {
    pub fn validate(&self) -> bool {
        self.duration_ms > 0
            && self.width > 0
            && self.height > 0
            && self.width <= 16_384
            && self.height <= 16_384
            && self.duration_ms <= 604_800_000
            && self.frame_rate.is_finite()
            && self.frame_rate > 0.0
            && self.frame_rate <= 1_000.0
            && (1..=64).contains(&self.video_codec.len())
            && self
                .audio_codec
                .as_ref()
                .is_none_or(|codec| codec.len() <= 64)
            && (1..=128).contains(&self.container.len())
            && (-360..=360).contains(&self.rotation_degrees)
    }
}
