use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use super::Database;
use crate::domain::{
    CoreError, NewTtsSegmentRun, TtsRunStatus, TtsSegmentRun, VoiceAssignment, VoiceScope,
};

#[derive(Clone)]
pub struct TtsRepository {
    database: Database,
}

impl TtsRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn set_assignment(
        &self,
        project_id: Uuid,
        scope: VoiceScope,
        provider_id: &str,
        voice_id: &str,
    ) -> Result<VoiceAssignment, CoreError> {
        if provider_id.is_empty()
            || voice_id.is_empty()
            || provider_id.len() > 128
            || voice_id.len() > 128
        {
            return Err(CoreError::InvalidInput("voice assignment"));
        }
        if let VoiceScope::Segment(id) = &scope {
            let exists: bool = self.database.connection()?.query_row(
                "SELECT EXISTS(SELECT 1 FROM segments WHERE id=?1 AND project_id=?2)",
                params![id.to_string(), project_id.to_string()],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(CoreError::NotFound("segment"));
            }
        }
        let id = Uuid::new_v4();
        let (kind, scope_id) = scope.to_storage();
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM voice_assignments WHERE project_id=?1 AND scope_type=?2 AND ((?3 IS NULL AND scope_id IS NULL) OR scope_id=?3)",params![project_id.to_string(),kind,scope_id])?;
        transaction.execute("INSERT INTO voice_assignments(id,project_id,scope_type,scope_id,provider_id,voice_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![id.to_string(),project_id.to_string(),kind,scope_id,provider_id,voice_id])?;
        transaction.commit()?;
        drop(connection);
        self.resolve_assignment(
            project_id,
            match scope {
                VoiceScope::Project => None,
                VoiceScope::Speaker(id) => Some((None, Some(id))),
                VoiceScope::Segment(id) => Some((Some(id), None)),
            },
        )?
        .ok_or(CoreError::NotFound("voice assignment"))
    }

    pub fn resolve_for_segment(
        &self,
        project_id: Uuid,
        segment_id: Uuid,
        speaker_id: Option<Uuid>,
    ) -> Result<Option<VoiceAssignment>, CoreError> {
        self.resolve_assignment(project_id, Some((Some(segment_id), speaker_id)))
    }

    pub fn list_assignments(&self, project_id: Uuid) -> Result<Vec<VoiceAssignment>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare("SELECT id,project_id,scope_type,scope_id,provider_id,voice_id FROM voice_assignments WHERE project_id=?1 ORDER BY scope_type,scope_id")?;
        let values = statement
            .query_map([project_id.to_string()], assignment_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    fn resolve_assignment(
        &self,
        project_id: Uuid,
        context: Option<(Option<Uuid>, Option<Uuid>)>,
    ) -> Result<Option<VoiceAssignment>, CoreError> {
        let (segment, speaker) = context.unwrap_or((None, None));
        let connection = self.database.connection()?;
        for (kind, id) in [
            ("segment", segment),
            ("speaker", speaker),
            ("project", None),
        ] {
            let value=connection.query_row("SELECT id,project_id,scope_type,scope_id,provider_id,voice_id FROM voice_assignments WHERE project_id=?1 AND scope_type=?2 AND ((?3 IS NULL AND scope_id IS NULL) OR scope_id=?3)",params![project_id.to_string(),kind,id.map(|v|v.to_string())],assignment_from_row).optional()?;
            if value.is_some() {
                return Ok(value);
            }
        }
        Ok(None)
    }

    pub fn insert_runs(&self, runs: &[NewTtsSegmentRun]) -> Result<Vec<TtsSegmentRun>, CoreError> {
        let mut connection = self.database.connection()?;
        let tx = connection.transaction()?;
        for run in runs {
            if run.cache_identity.len() != 64 || run.target_duration_ms == 0 {
                return Err(CoreError::InvalidInput("TTS segment run"));
            }
            let reused: Option<(String,i64,f64,Option<String>)>=tx.query_row("SELECT artifact_id,duration_ms,playback_rate,warning_code FROM tts_segment_runs WHERE project_id=?1 AND segment_id=?2 AND cache_identity=?3 AND status='completed' AND artifact_id IS NOT NULL ORDER BY updated_at DESC LIMIT 1",params![run.project_id.to_string(),run.segment_id.to_string(),run.cache_identity],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            let (status, artifact, duration, rate, warning) = match reused {
                Some((a, d, r, w)) => ("completed", Some(a), Some(d), Some(r), w),
                None => ("pending", None, None, None, None),
            };
            tx.execute("INSERT INTO tts_segment_runs(id,project_id,stage_run_id,segment_id,cache_identity,provider_id,voice_id,status,attempts,artifact_id,duration_ms,target_duration_ms,playback_rate,warning_code,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,?9,?10,?11,?12,?13,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![run.id.to_string(),run.project_id.to_string(),run.stage_run_id.to_string(),run.segment_id.to_string(),run.cache_identity,run.provider_id,run.voice_id,status,artifact,duration,run.target_duration_ms as i64,rate,warning])?;
        }
        tx.commit()?;
        drop(connection);
        self.list_for_stage(
            runs.first()
                .ok_or(CoreError::InvalidInput("TTS runs"))?
                .stage_run_id,
        )
    }

    pub fn list_for_stage(&self, stage_id: Uuid) -> Result<Vec<TtsSegmentRun>, CoreError> {
        let c = self.database.connection()?;
        let mut s = c.prepare(&format!(
            "{} WHERE stage_run_id=?1 ORDER BY rowid",
            select()
        ))?;
        let v = s
            .query_map([stage_id.to_string()], run_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(v)
    }
    pub fn get(&self, id: Uuid) -> Result<TtsSegmentRun, CoreError> {
        self.database
            .connection()?
            .query_row(
                &format!("{} WHERE id=?1", select()),
                [id.to_string()],
                run_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("TTS run"))
    }
    pub fn recover_stage(&self, id: Uuid) -> Result<usize, CoreError> {
        Ok(self.database.connection()?.execute("UPDATE tts_segment_runs SET status='pending',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE stage_run_id=?1 AND status='running'",[id.to_string()])?)
    }
    pub fn mark_running(&self, id: Uuid) -> Result<TtsSegmentRun, CoreError> {
        let c = self.database.connection()?;
        if c.execute("UPDATE tts_segment_runs SET status='running',attempts=attempts+1,error_code=NULL,safe_error_message=NULL WHERE id=?1 AND status IN('pending','failed')",[id.to_string()])?!=1{return Err(CoreError::InvalidTransition)}
        drop(c);
        self.get(id)
    }
    pub fn reset_pending(&self, id: Uuid) -> Result<(), CoreError> {
        self.database.connection()?.execute(
            "UPDATE tts_segment_runs SET status='pending' WHERE id=?1 AND status='running'",
            [id.to_string()],
        )?;
        Ok(())
    }
    pub fn invalidate_cached(&self, id: Uuid) -> Result<TtsSegmentRun, CoreError> {
        self.database.connection()?.execute("UPDATE tts_segment_runs SET status='pending',artifact_id=NULL,duration_ms=NULL,playback_rate=NULL,warning_code=NULL WHERE id=?1 AND status='completed'",[id.to_string()])?;
        self.get(id)
    }
    pub fn fail(&self, id: Uuid, code: &str) -> Result<(), CoreError> {
        self.database.connection()?.execute("UPDATE tts_segment_runs SET status='failed',error_code=?2,safe_error_message='Không thể tổng hợp đoạn thoại.' WHERE id=?1 AND status='running'",params![id.to_string(),code])?;
        Ok(())
    }
    pub fn complete(
        &self,
        id: Uuid,
        artifact_id: Uuid,
        duration: u64,
        rate: f64,
        warning: Option<&str>,
    ) -> Result<TtsSegmentRun, CoreError> {
        let run = self.get(id)?;
        let mut c = self.database.connection()?;
        let tx = c.transaction()?;
        tx.execute("UPDATE tts_segment_runs SET status='completed',artifact_id=?2,duration_ms=?3,playback_rate=?4,warning_code=?5,error_code=NULL,safe_error_message=NULL WHERE id=?1 AND status='running'",params![id.to_string(),artifact_id.to_string(),duration as i64,rate,warning])?;
        tx.execute("UPDATE segments SET voice_id=?2,estimated_duration_ms=?3,target_duration_ms=?4,playback_rate=?5,voice_hash=?6,audio_artifact_id=?7,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND project_id=?8",params![run.segment_id.to_string(),run.voice_id,duration as i64,run.target_duration_ms as i64,rate,run.cache_identity,artifact_id.to_string(),run.project_id.to_string()])?;
        tx.commit()?;
        drop(c);
        self.get(id)
    }
}

fn select() -> &'static str {
    "SELECT id,project_id,stage_run_id,segment_id,cache_identity,provider_id,voice_id,status,attempts,artifact_id,duration_ms,target_duration_ms,playback_rate,warning_code,error_code FROM tts_segment_runs"
}
fn parse(v: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&v).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn assignment_from_row(r: &Row<'_>) -> rusqlite::Result<VoiceAssignment> {
    let kind: String = r.get(2)?;
    let sid: Option<String> = r.get(3)?;
    let scope = match (kind.as_str(), sid) {
        ("project", None) => VoiceScope::Project,
        ("speaker", Some(v)) => VoiceScope::Speaker(parse(v)?),
        ("segment", Some(v)) => VoiceScope::Segment(parse(v)?),
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(VoiceAssignment {
        id: parse(r.get(0)?)?,
        project_id: parse(r.get(1)?)?,
        scope,
        provider_id: r.get(4)?,
        voice_id: r.get(5)?,
    })
}
fn run_from_row(r: &Row<'_>) -> rusqlite::Result<TtsSegmentRun> {
    let status: String = r.get(7)?;
    Ok(TtsSegmentRun {
        id: parse(r.get(0)?)?,
        project_id: parse(r.get(1)?)?,
        stage_run_id: parse(r.get(2)?)?,
        segment_id: parse(r.get(3)?)?,
        cache_identity: r.get(4)?,
        provider_id: r.get(5)?,
        voice_id: r.get(6)?,
        status: TtsRunStatus::from_storage(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        attempts: r.get(8)?,
        artifact_id: r.get::<_, Option<String>>(9)?.map(parse).transpose()?,
        duration_ms: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        target_duration_ms: r.get::<_, i64>(11)? as u64,
        playback_rate: r.get(12)?,
        warning_code: r.get(13)?,
        error_code: r.get(14)?,
    })
}
