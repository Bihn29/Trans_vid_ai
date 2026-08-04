use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::{CoreError, NewSegment, ReviewStatus, Segment, SegmentUpdate};

use super::Database;

#[derive(Clone)]
pub struct SegmentRepository {
    database: Database,
}

impl SegmentRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, segment: &NewSegment) -> Result<Segment, CoreError> {
        segment.validate()?;
        let source_hash = segment.source_hash();
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO segments(
                id, project_id, sequence, start_ms, end_ms, source_text,
                translated_text, speaker_id, voice_id, asr_confidence,
                estimated_duration_ms, target_duration_ms, playback_rate,
                enabled, review_status, source_hash, translation_hash, voice_hash,
                audio_artifact_id, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                '', ?7, NULL, ?8,
                NULL, NULL, 1.0,
                1, 'unreviewed', ?9, NULL, NULL,
                NULL,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![
                segment.id.to_string(),
                segment.project_id.to_string(),
                segment.sequence,
                segment.start_ms as i64,
                segment.end_ms as i64,
                segment.source_text,
                segment.speaker_id.map(|id| id.to_string()),
                segment.asr_confidence,
                source_hash,
            ],
        )?;
        drop(connection);
        self.get(segment.id)
    }

    pub fn bulk_insert(&self, segments: &[NewSegment]) -> Result<Vec<Segment>, CoreError> {
        for segment in segments {
            segment.validate()?;
        }
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        let mut statement = transaction.prepare(
            "INSERT INTO segments(
                id, project_id, sequence, start_ms, end_ms, source_text,
                translated_text, speaker_id, voice_id, asr_confidence,
                estimated_duration_ms, target_duration_ms, playback_rate,
                enabled, review_status, source_hash, translation_hash, voice_hash,
                audio_artifact_id, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                '', ?7, NULL, ?8,
                NULL, NULL, 1.0,
                1, 'unreviewed', ?9, NULL, NULL,
                NULL,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
        )?;
        for segment in segments {
            let source_hash = segment.source_hash();
            statement.execute(params![
                segment.id.to_string(),
                segment.project_id.to_string(),
                segment.sequence,
                segment.start_ms as i64,
                segment.end_ms as i64,
                segment.source_text,
                segment.speaker_id.map(|id| id.to_string()),
                segment.asr_confidence,
                source_hash,
            ])?;
        }
        drop(statement);
        transaction.commit()?;
        drop(connection);
        let ids: Vec<Uuid> = segments.iter().map(|s| s.id).collect();
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            result.push(self.get(id)?);
        }
        Ok(result)
    }

    pub fn get(&self, id: Uuid) -> Result<Segment, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT id, project_id, sequence, start_ms, end_ms, source_text,
                        translated_text, speaker_id, voice_id, asr_confidence,
                        estimated_duration_ms, target_duration_ms, playback_rate,
                        enabled, review_status, source_hash, translation_hash, voice_hash,
                        audio_artifact_id, created_at, updated_at
                 FROM segments WHERE id = ?1",
                [id.to_string()],
                segment_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("segment"))
    }

    pub fn get_for_project(&self, project_id: Uuid, id: Uuid) -> Result<Segment, CoreError> {
        let segment = self.get(id)?;
        if segment.project_id != project_id {
            return Err(CoreError::NotFound("segment"));
        }
        Ok(segment)
    }

    pub fn list_by_project(&self, project_id: Uuid) -> Result<Vec<Segment>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, sequence, start_ms, end_ms, source_text,
                    translated_text, speaker_id, voice_id, asr_confidence,
                    estimated_duration_ms, target_duration_ms, playback_rate,
                    enabled, review_status, source_hash, translation_hash, voice_hash,
                    audio_artifact_id, created_at, updated_at
             FROM segments WHERE project_id = ?1 ORDER BY sequence ASC",
        )?;
        let segments = statement
            .query_map([project_id.to_string()], segment_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(segments)
    }

    pub fn update(&self, id: Uuid, update: &SegmentUpdate) -> Result<Segment, CoreError> {
        let current = self.get(id)?;
        let start_ms = update.start_ms.unwrap_or(current.start_ms);
        let end_ms = update.end_ms.unwrap_or(current.end_ms);
        if end_ms <= start_ms {
            return Err(CoreError::SegmentOverlap);
        }
        let source_text = update
            .source_text
            .as_deref()
            .unwrap_or(&current.source_text);
        let translated_text = update
            .translated_text
            .as_deref()
            .unwrap_or(&current.translated_text);
        let speaker_id = update.speaker_id.unwrap_or(current.speaker_id);
        let voice_id = update.voice_id.clone().unwrap_or(current.voice_id);
        let enabled = update.enabled.unwrap_or(current.enabled);
        let review_status = update.review_status.unwrap_or(current.review_status);

        let source_hash = if update.source_text.is_some() {
            Some(crate::domain::compute_text_hash(source_text))
        } else {
            current.source_hash
        };
        let translation_hash = if update.translated_text.is_some() {
            Some(crate::domain::compute_text_hash(translated_text))
        } else {
            current.translation_hash
        };

        let connection = self.database.connection()?;
        let overlaps: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM segments
                WHERE project_id = ?1 AND id != ?2
                  AND start_ms < ?4 AND end_ms > ?3
             )",
            params![
                current.project_id.to_string(),
                id.to_string(),
                start_ms as i64,
                end_ms as i64
            ],
            |row| row.get(0),
        )?;
        if overlaps {
            return Err(CoreError::SegmentOverlap);
        }
        let changed = connection.execute(
            "UPDATE segments
             SET start_ms = ?2, end_ms = ?3,
                 source_text = ?4, translated_text = ?5, speaker_id = ?6,
                 voice_id = ?7, enabled = ?8, review_status = ?9,
                 source_hash = ?10, translation_hash = ?11,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                id.to_string(),
                start_ms as i64,
                end_ms as i64,
                source_text,
                translated_text,
                speaker_id.map(|v| v.to_string()),
                voice_id,
                enabled as i32,
                review_status.as_str(),
                source_hash,
                translation_hash,
            ],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("segment"));
        }
        drop(connection);
        self.get(id)
    }

    pub fn update_for_project(
        &self,
        project_id: Uuid,
        id: Uuid,
        update: &SegmentUpdate,
    ) -> Result<Segment, CoreError> {
        self.get_for_project(project_id, id)?;
        self.update(id, update)
    }

    pub fn replace_project(
        &self,
        project_id: Uuid,
        segments: &[NewSegment],
    ) -> Result<Vec<Segment>, CoreError> {
        for segment in segments {
            segment.validate()?;
            if segment.project_id != project_id {
                return Err(CoreError::InvalidInput("segment project"));
            }
        }

        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM segments WHERE project_id = ?1",
            [project_id.to_string()],
        )?;
        for segment in segments {
            insert_segment(&transaction, segment)?;
        }
        transaction.commit()?;
        drop(connection);
        self.list_by_project(project_id)
    }

    pub fn replace_one_with_two(
        &self,
        project_id: Uuid,
        original_id: Uuid,
        first: &NewSegment,
        second: &NewSegment,
    ) -> Result<(), CoreError> {
        first.validate()?;
        second.validate()?;
        if first.project_id != project_id || second.project_id != project_id {
            return Err(CoreError::InvalidInput("segment project"));
        }

        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE segments SET sequence = sequence + 1000000 WHERE project_id = ?1",
            [project_id.to_string()],
        )?;
        let changed = transaction.execute(
            "DELETE FROM segments WHERE id = ?1 AND project_id = ?2",
            params![original_id.to_string(), project_id.to_string()],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("segment"));
        }
        insert_segment(&transaction, first)?;
        insert_segment(&transaction, second)?;
        resequence_segments(&transaction, project_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_two_with_one(
        &self,
        project_id: Uuid,
        first_id: Uuid,
        second_id: Uuid,
        merged: &NewSegment,
    ) -> Result<(), CoreError> {
        merged.validate()?;
        if first_id == second_id || merged.project_id != project_id {
            return Err(CoreError::InvalidInput("merge segments"));
        }

        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE segments SET sequence = sequence + 1000000 WHERE project_id = ?1",
            [project_id.to_string()],
        )?;
        let changed = transaction.execute(
            "DELETE FROM segments
             WHERE project_id = ?1 AND id IN (?2, ?3)",
            params![
                project_id.to_string(),
                first_id.to_string(),
                second_id.to_string()
            ],
        )?;
        if changed != 2 {
            return Err(CoreError::NotFound("segment"));
        }
        insert_segment(&transaction, merged)?;
        resequence_segments(&transaction, project_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_region(
        &self,
        project_id: Uuid,
        start_ms: u64,
        end_ms: u64,
        segments: &[NewSegment],
    ) -> Result<Vec<Segment>, CoreError> {
        if end_ms <= start_ms {
            return Err(CoreError::InvalidInput("ASR rerun region"));
        }
        for segment in segments {
            segment.validate()?;
            if segment.project_id != project_id
                || segment.start_ms < start_ms
                || segment.end_ms > end_ms
            {
                return Err(CoreError::InvalidInput("regional ASR segment"));
            }
        }

        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE segments SET sequence = sequence + 1000000 WHERE project_id = ?1",
            [project_id.to_string()],
        )?;
        let changed = transaction.execute(
            "DELETE FROM segments
             WHERE project_id = ?1 AND start_ms < ?3 AND end_ms > ?2",
            params![project_id.to_string(), start_ms as i64, end_ms as i64],
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound("segments in ASR region"));
        }
        for segment in segments {
            insert_segment(&transaction, segment)?;
        }
        resequence_segments(&transaction, project_id)?;
        transaction.commit()?;
        drop(connection);
        self.list_by_project(project_id)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute("DELETE FROM segments WHERE id = ?1", [id.to_string()])?;
        if changed != 1 {
            return Err(CoreError::NotFound("segment"));
        }
        Ok(())
    }

    pub fn delete_by_project(&self, project_id: Uuid) -> Result<usize, CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "DELETE FROM segments WHERE project_id = ?1",
            [project_id.to_string()],
        )?;
        Ok(changed)
    }

    pub fn resequence(&self, project_id: Uuid) -> Result<(), CoreError> {
        let connection = self.database.connection()?;
        resequence_segments(&connection, project_id)
    }
}

fn insert_segment(
    connection: &rusqlite::Connection,
    segment: &NewSegment,
) -> Result<(), CoreError> {
    let source_hash = segment.source_hash();
    connection.execute(
        "INSERT INTO segments(
            id, project_id, sequence, start_ms, end_ms, source_text,
            translated_text, speaker_id, voice_id, asr_confidence,
            estimated_duration_ms, target_duration_ms, playback_rate,
            enabled, review_status, source_hash, translation_hash, voice_hash,
            audio_artifact_id, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            '', ?7, NULL, ?8,
            NULL, NULL, 1.0,
            1, 'unreviewed', ?9, NULL, NULL,
            NULL,
            strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         )",
        params![
            segment.id.to_string(),
            segment.project_id.to_string(),
            segment.sequence,
            segment.start_ms as i64,
            segment.end_ms as i64,
            segment.source_text,
            segment.speaker_id.map(|id| id.to_string()),
            segment.asr_confidence,
            source_hash,
        ],
    )?;
    Ok(())
}

fn resequence_segments(
    connection: &rusqlite::Connection,
    project_id: Uuid,
) -> Result<(), CoreError> {
    let mut statement = connection.prepare(
        "SELECT id FROM segments WHERE project_id = ?1 ORDER BY start_ms ASC, sequence ASC",
    )?;
    let ids = statement
        .query_map([project_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    connection.execute(
        "UPDATE segments SET sequence = sequence + 1000000 WHERE project_id = ?1",
        [project_id.to_string()],
    )?;

    for (index, id) in ids.iter().enumerate() {
        connection.execute(
            "UPDATE segments SET sequence = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            params![id, index as u32],
        )?;
    }
    Ok(())
}

fn segment_from_row(row: &Row<'_>) -> rusqlite::Result<Segment> {
    let id: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let sequence: u32 = row.get(2)?;
    let start_ms: i64 = row.get(3)?;
    let end_ms: i64 = row.get(4)?;
    let speaker_id: Option<String> = row.get(7)?;
    let asr_confidence: Option<f64> = row.get(9)?;
    let estimated_duration_ms: Option<i64> = row.get(10)?;
    let target_duration_ms: Option<i64> = row.get(11)?;
    let playback_rate: f64 = row.get(12)?;
    let enabled: i32 = row.get(13)?;
    let review_status: String = row.get(14)?;
    let audio_artifact_id: Option<String> = row.get(18)?;

    Ok(Segment {
        id: parse_uuid(&id)?,
        project_id: parse_uuid(&project_id)?,
        sequence,
        start_ms: start_ms as u64,
        end_ms: end_ms as u64,
        source_text: row.get(5)?,
        translated_text: row.get(6)?,
        speaker_id: speaker_id.as_deref().map(parse_uuid).transpose()?,
        voice_id: row.get(8)?,
        asr_confidence,
        estimated_duration_ms: estimated_duration_ms.map(|v| v as u64),
        target_duration_ms: target_duration_ms.map(|v| v as u64),
        playback_rate,
        enabled: enabled != 0,
        review_status: ReviewStatus::from_storage(&review_status)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        source_hash: row.get(15)?,
        translation_hash: row.get(16)?,
        voice_hash: row.get(17)?,
        audio_artifact_id: audio_artifact_id.as_deref().map(parse_uuid).transpose()?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
