use rusqlite::params;
use uuid::Uuid;

use crate::{
    domain::{CoreError, StageName, StageScope, StageStatus},
    persistence::{Database, StageRunRepository},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidationChange {
    SourceTranscript {
        segment_id: Uuid,
    },
    TranslationText {
        segment_id: Uuid,
    },
    VoiceAssignment {
        segment_ids: Vec<Uuid>,
        speaker_id: Option<Uuid>,
    },
    AudioMix,
    SubtitleStyle,
    Composition,
}

#[derive(Clone)]
pub struct InvalidationEngine {
    database: Database,
    stages: StageRunRepository,
}

impl InvalidationEngine {
    pub fn new(database: Database) -> Self {
        Self {
            stages: StageRunRepository::new(database.clone()),
            database,
        }
    }

    pub fn invalidate(
        &self,
        project_id: Uuid,
        change: &InvalidationChange,
    ) -> Result<Vec<Uuid>, CoreError> {
        let runs = self.stages.list_for_project(project_id)?;
        let affected = runs
            .iter()
            .filter(|run| should_invalidate(run.stage_name, &run.scope, change))
            .collect::<Vec<_>>();
        if affected
            .iter()
            .any(|run| run.status == StageStatus::Running)
        {
            return Err(CoreError::RunningStageConflict);
        }
        let ids = affected.iter().map(|run| run.stage_id).collect::<Vec<_>>();
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        for id in &ids {
            transaction.execute(
                "UPDATE stage_runs
                 SET status = 'invalidated',
                     completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     error_code = NULL, safe_error_message = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE stage_id = ?1 AND status != 'running'",
                params![id.to_string()],
            )?;
        }
        transaction.commit()?;
        Ok(ids)
    }
}

fn should_invalidate(stage: StageName, scope: &StageScope, change: &InvalidationChange) -> bool {
    let project_tail = matches!(
        stage,
        StageName::MixAudio
            | StageName::ComposeVideo
            | StageName::QualityCheck
            | StageName::Render
            | StageName::Complete
    );
    match change {
        InvalidationChange::SourceTranscript { segment_id } => {
            is_segment_stage(
                stage,
                scope,
                *segment_id,
                &[
                    StageName::Translate,
                    StageName::TranslationReview,
                    StageName::Synthesize,
                    StageName::FitDuration,
                ],
            ) || (stage == StageName::TranslationReview && matches!(scope, StageScope::Project))
                || project_tail
        }
        InvalidationChange::TranslationText { segment_id } => {
            is_segment_stage(
                stage,
                scope,
                *segment_id,
                &[StageName::Synthesize, StageName::FitDuration],
            ) || project_tail
        }
        InvalidationChange::VoiceAssignment {
            segment_ids,
            speaker_id,
        } => {
            let scoped_voice = matches!(scope, StageScope::Segment(id) if segment_ids.contains(id))
                || matches!((scope, speaker_id), (StageScope::Speaker(id), Some(changed)) if id == changed);
            let project_voice = matches!(scope, StageScope::Project) && !segment_ids.is_empty();
            ((scoped_voice || project_voice)
                && matches!(stage, StageName::Synthesize | StageName::FitDuration))
                || project_tail
        }
        InvalidationChange::AudioMix => project_tail,
        InvalidationChange::SubtitleStyle | InvalidationChange::Composition => matches!(
            stage,
            StageName::ComposeVideo
                | StageName::QualityCheck
                | StageName::Render
                | StageName::Complete
        ),
    }
}

fn is_segment_stage(
    stage: StageName,
    scope: &StageScope,
    segment_id: Uuid,
    stages: &[StageName],
) -> bool {
    stages.contains(&stage) && matches!(scope, StageScope::Segment(id) if *id == segment_id)
}
