use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Unreviewed,
    Approved,
    NeedsAttention,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreviewed => "unreviewed",
            Self::Approved => "approved",
            Self::NeedsAttention => "needs_attention",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "unreviewed" => Ok(Self::Unreviewed),
            "approved" => Ok(Self::Approved),
            "needs_attention" => Ok(Self::NeedsAttention),
            _ => Err(CoreError::InvalidInput("stored review status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub sequence: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
    pub speaker_id: Option<Uuid>,
    pub voice_id: Option<String>,
    pub asr_confidence: Option<f64>,
    pub estimated_duration_ms: Option<u64>,
    pub target_duration_ms: Option<u64>,
    pub playback_rate: f64,
    pub enabled: bool,
    pub review_status: ReviewStatus,
    pub source_hash: Option<String>,
    pub translation_hash: Option<String>,
    pub voice_hash: Option<String>,
    pub audio_artifact_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewSegment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub sequence: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub speaker_id: Option<Uuid>,
    pub asr_confidence: Option<f64>,
}

impl NewSegment {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.end_ms <= self.start_ms {
            return Err(CoreError::SegmentOverlap);
        }
        Ok(())
    }

    pub fn source_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.source_text.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SegmentUpdate {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub source_text: Option<String>,
    pub translated_text: Option<String>,
    pub speaker_id: Option<Option<Uuid>>,
    pub voice_id: Option<Option<String>>,
    pub enabled: Option<bool>,
    pub review_status: Option<ReviewStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SegmentWarning {
    Overlap { segment_a: Uuid, segment_b: Uuid },
    EmptyText { segment_id: Uuid },
    LongSegment { segment_id: Uuid, duration_ms: u64 },
    Repetition { segment_id: Uuid },
    Silence { after_segment_id: Uuid, gap_ms: u64 },
    LowConfidence { segment_id: Uuid, confidence: u32 },
}

const MAX_SEGMENT_DURATION_MS: u64 = 15_000;
const LOW_CONFIDENCE_THRESHOLD: f64 = 0.3;
const SILENCE_THRESHOLD_MS: u64 = 5_000;

pub fn check_transcript_quality(segments: &[Segment]) -> Vec<SegmentWarning> {
    let mut warnings = Vec::new();

    for segment in segments {
        if segment.source_text.trim().is_empty() {
            warnings.push(SegmentWarning::EmptyText {
                segment_id: segment.id,
            });
        }

        let duration = segment.end_ms.saturating_sub(segment.start_ms);
        if duration > MAX_SEGMENT_DURATION_MS {
            warnings.push(SegmentWarning::LongSegment {
                segment_id: segment.id,
                duration_ms: duration,
            });
        }

        if let Some(confidence) = segment.asr_confidence {
            if confidence < LOW_CONFIDENCE_THRESHOLD {
                warnings.push(SegmentWarning::LowConfidence {
                    segment_id: segment.id,
                    confidence: (confidence * 100.0) as u32,
                });
            }
        }
    }

    for window in segments.windows(2) {
        let (a, b) = (&window[0], &window[1]);

        if a.end_ms > b.start_ms {
            warnings.push(SegmentWarning::Overlap {
                segment_a: a.id,
                segment_b: b.id,
            });
        }

        let gap = b.start_ms.saturating_sub(a.end_ms);
        if gap > SILENCE_THRESHOLD_MS {
            warnings.push(SegmentWarning::Silence {
                after_segment_id: a.id,
                gap_ms: gap,
            });
        }

        if !a.source_text.trim().is_empty() && a.source_text.trim() == b.source_text.trim() {
            warnings.push(SegmentWarning::Repetition { segment_id: b.id });
        }
    }

    warnings
}

pub fn compute_text_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segment(
        id: Uuid,
        seq: u32,
        start: u64,
        end: u64,
        text: &str,
        confidence: Option<f64>,
    ) -> Segment {
        Segment {
            id,
            project_id: Uuid::new_v4(),
            sequence: seq,
            start_ms: start,
            end_ms: end,
            source_text: text.into(),
            translated_text: String::new(),
            speaker_id: None,
            voice_id: None,
            asr_confidence: confidence,
            estimated_duration_ms: None,
            target_duration_ms: None,
            playback_rate: 1.0,
            enabled: true,
            review_status: ReviewStatus::Unreviewed,
            source_hash: None,
            translation_hash: None,
            voice_hash: None,
            audio_artifact_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn detects_overlap_between_adjacent_segments() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let segments = vec![
            make_segment(a, 0, 0, 3000, "hello", Some(0.9)),
            make_segment(b, 1, 2500, 5000, "world", Some(0.9)),
        ];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SegmentWarning::Overlap { .. })));
    }

    #[test]
    fn detects_empty_text() {
        let id = Uuid::new_v4();
        let segments = vec![make_segment(id, 0, 0, 1000, "  ", Some(0.9))];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SegmentWarning::EmptyText { .. })));
    }

    #[test]
    fn detects_long_segment() {
        let id = Uuid::new_v4();
        let segments = vec![make_segment(id, 0, 0, 20_000, "long text", Some(0.9))];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings.iter().any(|w| matches!(w, SegmentWarning::LongSegment { duration_ms, .. } if *duration_ms == 20_000)));
    }

    #[test]
    fn detects_low_confidence() {
        let id = Uuid::new_v4();
        let segments = vec![make_segment(id, 0, 0, 1000, "text", Some(0.1))];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SegmentWarning::LowConfidence { .. })));
    }

    #[test]
    fn detects_silence_gap() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let segments = vec![
            make_segment(a, 0, 0, 1000, "first", Some(0.9)),
            make_segment(b, 1, 7000, 8000, "second", Some(0.9)),
        ];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SegmentWarning::Silence { gap_ms, .. } if *gap_ms == 6000)));
    }

    #[test]
    fn detects_repetition() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let segments = vec![
            make_segment(a, 0, 0, 1000, "same text", Some(0.9)),
            make_segment(b, 1, 1000, 2000, "same text", Some(0.9)),
        ];
        let warnings = check_transcript_quality(&segments);
        assert!(warnings
            .iter()
            .any(|w| matches!(w, SegmentWarning::Repetition { .. })));
    }

    #[test]
    fn source_hash_is_deterministic() {
        let hash1 = compute_text_hash("hello world");
        let hash2 = compute_text_hash("hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }
}
