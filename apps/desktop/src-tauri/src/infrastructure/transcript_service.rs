use uuid::Uuid;

use crate::domain::{
    check_transcript_quality, compute_text_hash, CoreError, NewSegment, ReviewStatus, Segment,
    SegmentUpdate, SegmentWarning, StageName, StageScope,
};
use crate::jobs::{InvalidationChange, InvalidationEngine};
use crate::persistence::SegmentRepository;

/// Service for transcript segment operations including import, editing,
/// split/merge, QC, and review checkpoint support.
#[derive(Clone)]
pub struct TranscriptService {
    segments: SegmentRepository,
    invalidation: InvalidationEngine,
}

impl TranscriptService {
    pub fn new(segments: SegmentRepository, invalidation: InvalidationEngine) -> Self {
        Self {
            segments,
            invalidation,
        }
    }

    /// Import ASR results as segments, replacing any existing segments for the project.
    pub fn import_asr_results(
        &self,
        project_id: Uuid,
        raw_segments: Vec<NewSegment>,
    ) -> Result<Vec<Segment>, CoreError> {
        if self
            .segments
            .list_by_project(project_id)?
            .iter()
            .any(|segment| segment.review_status == ReviewStatus::Approved)
        {
            return Err(CoreError::TranscriptLocked);
        }
        for segment in &raw_segments {
            segment.validate()?;
            if segment.project_id != project_id {
                return Err(CoreError::InvalidInput("segment project"));
            }
        }

        // Validate no overlaps in the incoming segments
        let mut sorted = raw_segments.clone();
        sorted.sort_by_key(|s| s.start_ms);
        for window in sorted.windows(2) {
            if window[0].end_ms > window[1].start_ms {
                return Err(CoreError::SegmentOverlap);
            }
        }

        self.segments.replace_project(project_id, &sorted)
    }

    /// Get all segments for a project in sequence order.
    pub fn get_transcript(&self, project_id: Uuid) -> Result<Vec<Segment>, CoreError> {
        self.segments.list_by_project(project_id)
    }

    /// Get a single segment.
    pub fn get_segment(&self, project_id: Uuid, segment_id: Uuid) -> Result<Segment, CoreError> {
        self.segments.get_for_project(project_id, segment_id)
    }

    /// Update the source text of a segment.
    pub fn update_segment_text(
        &self,
        project_id: Uuid,
        segment_id: Uuid,
        source_text: String,
    ) -> Result<Segment, CoreError> {
        let current = self.segments.get_for_project(project_id, segment_id)?;
        if current.review_status == ReviewStatus::Approved {
            return Err(CoreError::TranscriptLocked);
        }
        let update = SegmentUpdate {
            source_text: Some(source_text),
            ..Default::default()
        };
        self.invalidation.invalidate(
            project_id,
            &InvalidationChange::SourceTranscript {
                segment_id: current.id,
            },
        )?;
        self.segments
            .update_for_project(project_id, segment_id, &update)
    }

    /// Update a segment with arbitrary fields.
    pub fn update_segment(
        &self,
        project_id: Uuid,
        segment_id: Uuid,
        update: &SegmentUpdate,
    ) -> Result<Segment, CoreError> {
        let current = self.segments.get_for_project(project_id, segment_id)?;
        if update.source_text.is_some()
            || update.enabled.is_some()
            || update.start_ms.is_some()
            || update.end_ms.is_some()
        {
            self.invalidation.invalidate(
                project_id,
                &InvalidationChange::SourceTranscript {
                    segment_id: current.id,
                },
            )?;
        } else if update.translated_text.is_some() {
            self.invalidation.invalidate(
                project_id,
                &InvalidationChange::TranslationText {
                    segment_id: current.id,
                },
            )?;
        } else if update.speaker_id.is_some() {
            self.invalidation.invalidate(
                project_id,
                &InvalidationChange::VoiceAssignment {
                    segment_ids: vec![current.id],
                    speaker_id: current.speaker_id,
                },
            )?;
        }

        let mut normalized = update.clone();
        if update.source_text.is_some()
            || update.start_ms.is_some()
            || update.end_ms.is_some()
            || update.speaker_id.is_some()
        {
            normalized.review_status = Some(ReviewStatus::Unreviewed);
        }
        self.segments
            .update_for_project(project_id, segment_id, &normalized)
    }

    /// Split a segment at the given timestamp.
    ///
    /// Returns the two resulting segments. The original segment is deleted
    /// and replaced with two new segments that preserve the invariant
    /// `start_ms < split_ms < end_ms`.
    pub fn split_segment(
        &self,
        project_id: Uuid,
        segment_id: Uuid,
        split_ms: u64,
    ) -> Result<(Segment, Segment), CoreError> {
        let original = self.segments.get_for_project(project_id, segment_id)?;

        if split_ms <= original.start_ms || split_ms >= original.end_ms {
            return Err(CoreError::InvalidInput("split point"));
        }

        // Split text approximately by ratio
        let text = &original.source_text;
        if text.chars().count() < 2 {
            return Err(CoreError::InvalidInput("segment text for split"));
        }
        let ratio =
            (split_ms - original.start_ms) as f64 / (original.end_ms - original.start_ms) as f64;
        let chars: Vec<char> = text.chars().collect();
        let split_char = (chars.len() as f64 * ratio).round() as usize;
        let split_char = split_char.clamp(1, chars.len().saturating_sub(1));
        let first_text: String = chars[..split_char].iter().collect();
        let second_text: String = chars[split_char..].iter().collect();

        let first = NewSegment {
            id: Uuid::new_v4(),
            project_id: original.project_id,
            sequence: 0,
            start_ms: original.start_ms,
            end_ms: split_ms,
            source_text: first_text,
            speaker_id: original.speaker_id,
            asr_confidence: original.asr_confidence,
        };

        let second = NewSegment {
            id: Uuid::new_v4(),
            project_id: original.project_id,
            sequence: 1,
            start_ms: split_ms,
            end_ms: original.end_ms,
            source_text: second_text,
            speaker_id: original.speaker_id,
            asr_confidence: original.asr_confidence,
        };

        self.invalidation.invalidate(
            project_id,
            &InvalidationChange::SourceTranscript {
                segment_id: original.id,
            },
        )?;
        self.segments
            .replace_one_with_two(project_id, segment_id, &first, &second)?;

        let first = self.segments.get_for_project(project_id, first.id)?;
        let second = self.segments.get_for_project(project_id, second.id)?;
        Ok((first, second))
    }

    /// Merge two adjacent segments.
    ///
    /// The merged segment uses the earliest start, latest end, and concatenated
    /// text. Both original segments are deleted and replaced with one new segment.
    pub fn merge_segments(
        &self,
        project_id: Uuid,
        segment_a_id: Uuid,
        segment_b_id: Uuid,
    ) -> Result<Segment, CoreError> {
        if segment_a_id == segment_b_id {
            return Err(CoreError::InvalidInput("merge segments"));
        }
        let a = self.segments.get_for_project(project_id, segment_a_id)?;
        let b = self.segments.get_for_project(project_id, segment_b_id)?;

        if a.sequence.abs_diff(b.sequence) != 1 {
            return Err(CoreError::InvalidInput("non-adjacent segments"));
        }

        let start_ms = a.start_ms.min(b.start_ms);
        let end_ms = a.end_ms.max(b.end_ms);
        let text = format!("{} {}", a.source_text.trim(), b.source_text.trim());
        let confidence = match (a.asr_confidence, b.asr_confidence) {
            (Some(ca), Some(cb)) => Some(ca.min(cb)),
            (Some(c), None) | (None, Some(c)) => Some(c),
            (None, None) => None,
        };

        let merged = NewSegment {
            id: Uuid::new_v4(),
            project_id: a.project_id,
            sequence: 0,
            start_ms,
            end_ms,
            source_text: text,
            speaker_id: a.speaker_id,
            asr_confidence: confidence,
        };

        for segment_id in [a.id, b.id] {
            self.invalidation.invalidate(
                project_id,
                &InvalidationChange::SourceTranscript { segment_id },
            )?;
        }
        self.segments
            .replace_two_with_one(project_id, segment_a_id, segment_b_id, &merged)?;

        self.segments.get_for_project(project_id, merged.id)
    }

    /// Run quality check on a project's transcript.
    pub fn check_quality(&self, project_id: Uuid) -> Result<Vec<SegmentWarning>, CoreError> {
        let segments = self.segments.list_by_project(project_id)?;
        Ok(check_transcript_quality(&segments))
    }

    /// Approve transcript review for a project by marking all unreviewed segments as approved.
    pub fn approve_transcript(&self, project_id: Uuid) -> Result<Vec<Segment>, CoreError> {
        let segments = self.segments.list_by_project(project_id)?;
        for segment in &segments {
            if segment.review_status == ReviewStatus::Unreviewed {
                self.segments.update(
                    segment.id,
                    &SegmentUpdate {
                        review_status: Some(ReviewStatus::Approved),
                        ..Default::default()
                    },
                )?;
            }
        }
        self.segments.list_by_project(project_id)
    }

    pub fn prepare_regional_rerun(
        &self,
        project_id: Uuid,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Vec<Segment>, CoreError> {
        if end_ms <= start_ms {
            return Err(CoreError::InvalidInput("ASR rerun region"));
        }
        let affected = self
            .segments
            .list_by_project(project_id)?
            .into_iter()
            .filter(|segment| segment.start_ms < end_ms && segment.end_ms > start_ms)
            .collect::<Vec<_>>();
        if affected.is_empty() {
            return Err(CoreError::NotFound("segments in ASR region"));
        }
        for segment in &affected {
            self.invalidation.invalidate(
                project_id,
                &InvalidationChange::SourceTranscript {
                    segment_id: segment.id,
                },
            )?;
        }
        Ok(affected)
    }

    pub fn replace_regional_asr_results(
        &self,
        project_id: Uuid,
        start_ms: u64,
        end_ms: u64,
        raw_segments: Vec<NewSegment>,
    ) -> Result<Vec<Segment>, CoreError> {
        let mut sorted = raw_segments;
        sorted.sort_by_key(|segment| segment.start_ms);
        for segment in &sorted {
            segment.validate()?;
            if segment.project_id != project_id
                || segment.start_ms < start_ms
                || segment.end_ms > end_ms
            {
                return Err(CoreError::InvalidInput("regional ASR segment"));
            }
        }
        if sorted
            .windows(2)
            .any(|window| window[0].end_ms > window[1].start_ms)
        {
            return Err(CoreError::SegmentOverlap);
        }
        self.prepare_regional_rerun(project_id, start_ms, end_ms)?;
        self.segments
            .replace_region(project_id, start_ms, end_ms, &sorted)
    }

    /// Compute the source hash for a text for use in invalidation comparisons.
    pub fn compute_source_hash(text: &str) -> String {
        compute_text_hash(text)
    }

    /// Get the stage scopes that should be invalidated when a segment's source text changes.
    pub fn invalidation_scopes_for_source_edit(segment: &Segment) -> Vec<(StageName, StageScope)> {
        let segment_scope = StageScope::Segment(segment.id);
        vec![
            (StageName::Translate, segment_scope.clone()),
            (StageName::TranslationReview, StageScope::Project),
            (StageName::Synthesize, segment_scope.clone()),
            (StageName::FitDuration, segment_scope),
            (StageName::MixAudio, StageScope::Project),
            (StageName::ComposeVideo, StageScope::Project),
            (StageName::Render, StageScope::Project),
        ]
    }
}
