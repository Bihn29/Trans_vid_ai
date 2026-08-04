use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{ArtifactRegistry, ProjectLayout, ProjectRelativePath, TranscriptService};
use crate::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, AudioMixSettings, AudioQualityReport,
        CoreError, SeparationEngineDescriptor, StageName,
    },
    jobs::{ClaimedJob, PersistentQueue},
    workers::{ArtifactOutput, WorkerManager, WorkerRequest},
};

const MAX_WAV_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SeparationExecutionRequest {
    pub source_artifact_id: Uuid,
    pub engine: SeparationEngineDescriptor,
    pub energy_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct SeparationExecutionResult {
    pub vocals: Artifact,
    pub background: Artifact,
    pub mode: String,
}

#[derive(Debug, Clone)]
pub struct AudioMixRequest {
    pub background_artifact_id: Uuid,
    pub original_voice_artifact_id: Option<Uuid>,
    pub music_artifact_id: Option<Uuid>,
    pub settings: AudioMixSettings,
}

#[derive(Debug, Clone)]
pub struct AudioMixResult {
    pub artifact: Artifact,
    pub quality: AudioQualityReport,
}

#[derive(Clone)]
pub struct AudioPipelineService {
    artifacts: ArtifactRegistry,
    layout: ProjectLayout,
    transcript: TranscriptService,
    workers: WorkerManager,
}

impl AudioPipelineService {
    pub fn new(
        artifacts: ArtifactRegistry,
        layout: ProjectLayout,
        transcript: TranscriptService,
        workers: WorkerManager,
    ) -> Self {
        Self {
            artifacts,
            layout,
            transcript,
            workers,
        }
    }

    pub async fn execute_separation_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        request: &SeparationExecutionRequest,
    ) -> Result<SeparationExecutionResult, CoreError> {
        if claimed.job.job_type != StageName::SeparateAudio || !valid_engine(request) {
            let error = CoreError::InvalidInput("separation execution");
            queue.fail(
                claimed.job.id,
                error.code(),
                "Cấu hình tách âm không hợp lệ.",
            )?;
            return Err(error);
        }
        let source = match self.verified_artifact(
            claimed.job.project_id,
            request.source_artifact_id,
            &[ArtifactKind::OriginalAudio],
        ) {
            Ok(value) => value,
            Err(error) => {
                queue.fail(claimed.job.id, error.code(), "Âm thanh nguồn không hợp lệ.")?;
                return Err(error);
            }
        };
        let root = match self.layout.project_root(claimed.job.project_id) {
            Ok(value) => value,
            Err(error) => {
                queue.fail(claimed.job.id, error.code(), "Thư mục dự án không hợp lệ.")?;
                return Err(error);
            }
        };
        let mut worker_request =
            WorkerRequest::new("separate_audio", claimed.job.project_id, "audio/background");
        worker_request.input.insert(
            "separation".into(),
            json!({
                "schema_version": 1,
                "source_relative_path": source.relative_path,
                "engine_id": request.engine.engine_id,
                "energy_threshold": request.energy_threshold,
            }),
        );
        let worker_result =
            match self
                .workers
                .client_for_stage(StageName::SeparateAudio, &root, &[])
            {
                Ok(client) => client
                    .run(&worker_request, claimed.cancellation.clone())
                    .await
                    .ok(),
                Err(_) => None,
            };
        if claimed.cancellation.is_cancelled() {
            queue.acknowledge_interruption(claimed.job.id)?;
            return Err(CoreError::WorkerExecution);
        }
        let result = worker_result
            .and_then(|output| {
                self.consume_separation(claimed.job.project_id, &output.artifacts)
                    .ok()
            })
            .map(Ok)
            .unwrap_or_else(|| self.fallback_separation(claimed.job.project_id, &source));
        let result = match result {
            Ok(value) => value,
            Err(error) => {
                queue.fail(
                    claimed.job.id,
                    error.code(),
                    "Không thể tách hoặc giảm nền âm thanh.",
                )?;
                return Err(error);
            }
        };
        queue.complete(claimed.job.id, &[result.vocals.id, result.background.id])?;
        Ok(result)
    }

    pub fn execute_mix_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        request: &AudioMixRequest,
    ) -> Result<AudioMixResult, CoreError> {
        if claimed.job.job_type != StageName::MixAudio
            || request.settings.project_id != claimed.job.project_id
            || request.settings.validate().is_err()
        {
            let error = CoreError::InvalidInput("audio mix execution");
            queue.fail(
                claimed.job.id,
                error.code(),
                "Cấu hình phối âm không hợp lệ.",
            )?;
            return Err(error);
        }
        let mixed = self.mix(claimed.job.project_id, request, &claimed.cancellation);
        if claimed.cancellation.is_cancelled() {
            queue.acknowledge_interruption(claimed.job.id)?;
            return Err(CoreError::WorkerExecution);
        }
        let result = match mixed {
            Ok(value) => value,
            Err(error) => {
                queue.fail(claimed.job.id, error.code(), "Không thể phối âm thanh.")?;
                return Err(error);
            }
        };
        queue.complete(claimed.job.id, &[result.artifact.id])?;
        Ok(result)
    }

    fn consume_separation(
        &self,
        project_id: Uuid,
        outputs: &[ArtifactOutput],
    ) -> Result<SeparationExecutionResult, CoreError> {
        if outputs.len() != 2 {
            return Err(CoreError::InvalidInput("separation artifacts"));
        }
        let vocals = outputs
            .iter()
            .find(|output| output.r#type == "vocals")
            .ok_or(CoreError::InvalidInput("vocals artifact"))?;
        let background = outputs
            .iter()
            .find(|output| output.r#type == "background")
            .ok_or(CoreError::InvalidInput("background artifact"))?;
        let vocal_wav = self.verify_worker_wav(project_id, vocals)?;
        let background_wav = self.verify_worker_wav(project_id, background)?;
        if vocal_wav.sample_rate != background_wav.sample_rate
            || vocal_wav.samples.len() != background_wav.samples.len()
        {
            return Err(CoreError::InvalidInput("separation alignment"));
        }
        let vocals_artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Vocals,
            &vocals.relative_path,
            StageName::SeparateAudio,
            &vocals.metadata,
        )?;
        let background_artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Background,
            &background.relative_path,
            StageName::SeparateAudio,
            &background.metadata,
        );
        let background_artifact = match background_artifact {
            Ok(value) => value,
            Err(error) => {
                self.artifacts.unregister(vocals_artifact.id)?;
                return Err(error);
            }
        };
        Ok(SeparationExecutionResult {
            vocals: vocals_artifact,
            background: background_artifact,
            mode: "separated".into(),
        })
    }

    fn fallback_separation(
        &self,
        project_id: Uuid,
        source: &Artifact,
    ) -> Result<SeparationExecutionResult, CoreError> {
        let source_wav = self.read_artifact_wav(source)?;
        let vocals_data = source_wav.to_bytes()?;
        let background_wav = PcmWav {
            sample_rate: source_wav.sample_rate,
            samples: source_wav
                .samples
                .iter()
                .map(|sample| (*sample as f32 * 0.25).round() as i16)
                .collect(),
        };
        let background_data = background_wav.to_bytes()?;
        let id = Uuid::new_v4();
        let vocals_relative = format!("audio/vocals/fallback-{id}.wav");
        let background_relative = format!("audio/background/fallback-{id}.wav");
        self.write_project_file(project_id, &vocals_relative, &vocals_data)?;
        if let Err(error) =
            self.write_project_file(project_id, &background_relative, &background_data)
        {
            self.remove_project_file(project_id, &vocals_relative);
            return Err(error);
        }
        let metadata = json!({
            "duration_ms": source_wav.duration_ms(),
            "sample_rate": source_wav.sample_rate,
            "channels": 1,
            "bits_per_sample": 16,
            "engine_id": "fallback-attenuation-v1",
            "separation_mode": "fallback_attenuation",
        })
        .as_object()
        .cloned()
        .ok_or(CoreError::InvalidInput("fallback metadata"))?;
        let vocals = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Vocals,
            &vocals_relative,
            StageName::SeparateAudio,
            &metadata,
        )?;
        let background = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Background,
            &background_relative,
            StageName::SeparateAudio,
            &metadata,
        );
        let background = match background {
            Ok(value) => value,
            Err(error) => {
                self.artifacts.unregister(vocals.id)?;
                return Err(error);
            }
        };
        Ok(SeparationExecutionResult {
            vocals,
            background,
            mode: "fallback_attenuation".into(),
        })
    }

    fn mix(
        &self,
        project_id: Uuid,
        request: &AudioMixRequest,
        cancellation: &CancellationToken,
    ) -> Result<AudioMixResult, CoreError> {
        let background_artifact = self.verified_artifact(
            project_id,
            request.background_artifact_id,
            &[ArtifactKind::Background],
        )?;
        let background = self.read_artifact_wav(&background_artifact)?;
        let mut bed = background
            .samples
            .iter()
            .map(|sample| *sample as f32 / 32768.0 * request.settings.background_gain)
            .collect::<Vec<_>>();
        if let Some(id) = request.original_voice_artifact_id {
            let artifact = self.verified_artifact(project_id, id, &[ArtifactKind::Vocals])?;
            add_lane(
                &mut bed,
                &self.read_artifact_wav(&artifact)?,
                background.sample_rate,
                request.settings.original_voice_gain,
            )?;
        }
        if let Some(id) = request.music_artifact_id {
            let artifact = self.verified_artifact(
                project_id,
                id,
                &[ArtifactKind::Music, ArtifactKind::Background],
            )?;
            add_lane(
                &mut bed,
                &self.read_artifact_wav(&artifact)?,
                background.sample_rate,
                request.settings.music_gain,
            )?;
        }
        let segments = self
            .transcript
            .get_transcript(project_id)?
            .into_iter()
            .filter(|segment| segment.enabled && segment.audio_artifact_id.is_some())
            .collect::<Vec<_>>();
        let mut voices = vec![0.0_f32; bed.len()];
        let mut active = vec![false; bed.len()];
        let mut timeline = Vec::new();
        for segment in segments {
            if cancellation.is_cancelled() {
                return Err(CoreError::WorkerExecution);
            }
            let artifact_id = segment
                .audio_artifact_id
                .ok_or(CoreError::NotFound("TTS artifact"))?;
            let artifact = self.verified_artifact(project_id, artifact_id, &[ArtifactKind::Tts])?;
            let wav = self.read_artifact_wav(&artifact)?;
            let start = ms_to_samples(segment.start_ms, background.sample_rate)?;
            let requested = ms_to_samples(
                segment.end_ms.saturating_sub(segment.start_ms),
                background.sample_rate,
            )?;
            let end = start.saturating_add(requested).min(voices.len());
            if start >= end {
                continue;
            }
            // Every voice clip is fitted to its transcript time slot, so the
            // destination sample count is authoritative. This also converts
            // provider output (for example MeloTTS at 44.1 kHz) to the project
            // mix rate (the normalized source is 16 kHz) in one interpolation.
            let fitted = resample(&wav.samples, end - start);
            let fade_in =
                ms_to_samples(request.settings.fade_in_ms as u64, background.sample_rate)?;
            let fade_out =
                ms_to_samples(request.settings.fade_out_ms as u64, background.sample_rate)?;
            for (offset, sample) in fitted.into_iter().enumerate() {
                let gain =
                    fade_gain(offset, end - start, fade_in, fade_out) * request.settings.voice_gain;
                voices[start + offset] += sample as f32 / 32768.0 * gain;
                active[start + offset] = true;
            }
            timeline.push(TimelineEntry {
                segment_id: segment.id,
                artifact_hash: artifact.sha256,
                start_sample: start,
                sample_count: end - start,
            });
        }
        for index in 0..bed.len() {
            if index % 65_536 == 0 && cancellation.is_cancelled() {
                return Err(CoreError::WorkerExecution);
            }
            if active[index] {
                bed[index] *= request.settings.ducking_gain;
            }
            bed[index] += voices[index];
        }
        normalize_rms(&mut bed, request.settings.target_rms_dbfs);
        let mut limited_samples = 0_u64;
        for sample in &mut bed {
            if sample.abs() > request.settings.limiter_peak {
                *sample = sample.clamp(
                    -request.settings.limiter_peak,
                    request.settings.limiter_peak,
                );
                limited_samples += 1;
            }
        }
        let pcm = bed
            .iter()
            .map(|sample| (sample.clamp(-0.999_969, 0.999_969) * 32768.0).round() as i16)
            .collect::<Vec<_>>();
        let timeline_hash = timeline_hash(&timeline, &request.settings)?;
        let peak = bed
            .iter()
            .fold(0.0_f32, |value, sample| value.max(sample.abs()));
        let rms = rms(&bed);
        let mixed = PcmWav {
            sample_rate: background.sample_rate,
            samples: pcm,
        };
        let quality = AudioQualityReport {
            duration_ms: mixed.duration_ms(),
            target_duration_ms: background.duration_ms(),
            peak_dbfs: amplitude_dbfs(peak),
            rms_dbfs: amplitude_dbfs(rms),
            clipped_samples: bed.iter().filter(|sample| sample.abs() >= 1.0).count() as u64,
            limited_samples,
            timeline_hash,
            separation_mode: background_artifact
                .metadata
                .get("separation_mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        };
        if !quality.passes() {
            return Err(CoreError::InvalidInput("audio quality"));
        }
        let id = Uuid::new_v4();
        let relative = format!("audio/mixed/mix-{id}.wav");
        self.write_project_file(project_id, &relative, &mixed.to_bytes()?)?;
        let metadata = serde_json::to_value(&quality)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .ok_or(CoreError::InvalidInput("audio quality metadata"))?;
        let artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::MixedAudio,
            &relative,
            StageName::MixAudio,
            &metadata,
        );
        match artifact {
            Ok(artifact) => Ok(AudioMixResult { artifact, quality }),
            Err(error) => {
                self.remove_project_file(project_id, &relative);
                Err(error)
            }
        }
    }

    fn verified_artifact(
        &self,
        project_id: Uuid,
        artifact_id: Uuid,
        kinds: &[ArtifactKind],
    ) -> Result<Artifact, CoreError> {
        let artifact = self.artifacts.get(artifact_id)?;
        if artifact.project_id != project_id || !kinds.contains(&artifact.kind) {
            return Err(CoreError::InvalidInput("audio artifact"));
        }
        if self.artifacts.verify(artifact_id)? != ArtifactVerification::Verified {
            return Err(CoreError::ArtifactIntegrity);
        }
        Ok(artifact)
    }

    fn read_artifact_wav(&self, artifact: &Artifact) -> Result<PcmWav, CoreError> {
        if artifact.size_bytes > MAX_WAV_BYTES {
            return Err(CoreError::InvalidInput("WAV size"));
        }
        let relative = ProjectRelativePath::parse(&artifact.relative_path)?;
        let path = self
            .layout
            .resolve_existing(artifact.project_id, &relative)?;
        PcmWav::parse(&fs::read(path)?)
    }

    fn verify_worker_wav(
        &self,
        project_id: Uuid,
        output: &ArtifactOutput,
    ) -> Result<PcmWav, CoreError> {
        if output.size_bytes > MAX_WAV_BYTES {
            return Err(CoreError::InvalidInput("WAV size"));
        }
        let relative = ProjectRelativePath::parse(&output.relative_path)?;
        let path = self.layout.resolve_existing(project_id, &relative)?;
        let bytes = fs::read(path)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if bytes.len() as u64 != output.size_bytes || hash != output.sha256 {
            return Err(CoreError::ArtifactIntegrity);
        }
        PcmWav::parse(&bytes)
    }

    fn write_project_file(
        &self,
        project_id: Uuid,
        relative: &str,
        bytes: &[u8],
    ) -> Result<(), CoreError> {
        let relative = ProjectRelativePath::parse(relative)?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let temporary = destination.with_file_name(format!(
            ".{}.{}.partial",
            destination
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or(CoreError::UnsafePath)?,
            Uuid::new_v4()
        ));
        let write = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(bytes)?;
                file.sync_all()
            })
            .and_then(|_| fs::rename(&temporary, &destination));
        if let Err(error) = write {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        Ok(())
    }

    fn remove_project_file(&self, project_id: Uuid, relative: &str) {
        if let Ok(relative) = ProjectRelativePath::parse(relative) {
            if let Ok(path) = self.layout.prepare_output(project_id, &relative) {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn valid_engine(request: &SeparationExecutionRequest) -> bool {
    request.engine.approved
        && request.engine.engine_id == "energy-mask-v1"
        && request.engine.version == "1.0.0"
        && request.engine.license == "UNLICENSED"
        && request.engine.install_mode == "bundled_source"
        && !request.engine.requires_consent
        && !request.engine.sends_data_off_device
        && request.energy_threshold.is_finite()
        && (0.01..=0.95).contains(&request.energy_threshold)
}

#[derive(Debug, Serialize)]
struct TimelineEntry {
    segment_id: Uuid,
    artifact_hash: String,
    start_sample: usize,
    sample_count: usize,
}

fn timeline_hash(
    entries: &[TimelineEntry],
    settings: &AudioMixSettings,
) -> Result<String, CoreError> {
    let value = serde_json::to_vec(&(entries, settings))
        .map_err(|_| CoreError::InvalidInput("audio timeline"))?;
    Ok(format!("{:x}", Sha256::digest(value)))
}

fn add_lane(
    destination: &mut [f32],
    source: &PcmWav,
    sample_rate: u32,
    gain: f32,
) -> Result<(), CoreError> {
    if source.sample_rate != sample_rate || source.samples.len() != destination.len() {
        return Err(CoreError::InvalidInput("audio sample rate"));
    }
    for (output, sample) in destination.iter_mut().zip(&source.samples) {
        *output += *sample as f32 / 32768.0 * gain;
    }
    Ok(())
}

fn ms_to_samples(value: u64, sample_rate: u32) -> Result<usize, CoreError> {
    usize::try_from(value.saturating_mul(sample_rate as u64) / 1_000)
        .map_err(|_| CoreError::InvalidInput("audio timeline"))
}

fn resample(source: &[i16], output_len: usize) -> Vec<i16> {
    if output_len == 0 || source.is_empty() {
        return Vec::new();
    }
    if output_len == 1 || source.len() == 1 {
        return vec![source[0]; output_len];
    }
    (0..output_len)
        .map(|index| {
            let position = index as f64 * (source.len() - 1) as f64 / (output_len - 1) as f64;
            let left = position.floor() as usize;
            let right = (left + 1).min(source.len() - 1);
            let fraction = (position - left as f64) as f32;
            (source[left] as f32 * (1.0 - fraction) + source[right] as f32 * fraction).round()
                as i16
        })
        .collect()
}

fn fade_gain(index: usize, length: usize, fade_in: usize, fade_out: usize) -> f32 {
    let mut gain = 1.0_f32;
    if fade_in > 0 && index < fade_in {
        gain = gain.min(index as f32 / fade_in as f32);
    }
    let remaining = length.saturating_sub(index + 1);
    if fade_out > 0 && remaining < fade_out {
        gain = gain.min(remaining as f32 / fade_out as f32);
    }
    gain
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

fn normalize_rms(samples: &mut [f32], target_dbfs: f32) {
    let current = rms(samples);
    if current > 0.0 {
        let target = 10_f32.powf(target_dbfs / 20.0);
        let gain = (target / current).clamp(0.25, 4.0);
        for sample in samples {
            *sample *= gain;
        }
    }
}

fn amplitude_dbfs(value: f32) -> f32 {
    if value <= 0.0 {
        -120.0
    } else {
        20.0 * value.log10()
    }
}

#[derive(Debug, Clone)]
struct PcmWav {
    sample_rate: u32,
    samples: Vec<i16>,
}

impl PcmWav {
    fn parse(data: &[u8]) -> Result<Self, CoreError> {
        if data.len() < 44
            || data.len() as u64 > MAX_WAV_BYTES
            || &data[..4] != b"RIFF"
            || &data[8..12] != b"WAVE"
        {
            return Err(CoreError::InvalidInput("PCM WAV"));
        }
        let mut offset = 12_usize;
        let mut format = None;
        let mut pcm = None;
        while offset + 8 <= data.len() {
            let kind = &data[offset..offset + 4];
            let length = u32::from_le_bytes(
                data[offset + 4..offset + 8]
                    .try_into()
                    .map_err(|_| CoreError::InvalidInput("PCM WAV"))?,
            ) as usize;
            let body = offset + 8;
            let end = body
                .checked_add(length)
                .ok_or(CoreError::InvalidInput("PCM WAV"))?;
            if end > data.len() {
                return Err(CoreError::InvalidInput("PCM WAV"));
            }
            if kind == b"fmt " && length >= 16 {
                format = Some((
                    u16::from_le_bytes(data[body..body + 2].try_into().unwrap()),
                    u16::from_le_bytes(data[body + 2..body + 4].try_into().unwrap()),
                    u32::from_le_bytes(data[body + 4..body + 8].try_into().unwrap()),
                    u16::from_le_bytes(data[body + 14..body + 16].try_into().unwrap()),
                ));
            } else if kind == b"data" {
                pcm = Some(&data[body..end]);
            }
            offset = end + length % 2;
        }
        let (encoding, channels, sample_rate, bits) =
            format.ok_or(CoreError::InvalidInput("PCM WAV"))?;
        let pcm = pcm.ok_or(CoreError::InvalidInput("PCM WAV"))?;
        if encoding != 1
            || channels != 1
            || bits != 16
            || !(8_000..=48_000).contains(&sample_rate)
            || pcm.is_empty()
            || pcm.len() % 2 != 0
        {
            return Err(CoreError::InvalidInput("PCM WAV"));
        }
        let samples = pcm
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        Ok(Self {
            sample_rate,
            samples,
        })
    }

    fn to_bytes(&self) -> Result<Vec<u8>, CoreError> {
        let data_size = self
            .samples
            .len()
            .checked_mul(2)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or(CoreError::InvalidInput("WAV size"))?;
        let mut data = Vec::with_capacity(data_size as usize + 44);
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&(36_u32 + data_size).to_le_bytes());
        data.extend_from_slice(b"WAVEfmt ");
        data.extend_from_slice(&16_u32.to_le_bytes());
        data.extend_from_slice(&1_u16.to_le_bytes());
        data.extend_from_slice(&1_u16.to_le_bytes());
        data.extend_from_slice(&self.sample_rate.to_le_bytes());
        data.extend_from_slice(&(self.sample_rate * 2).to_le_bytes());
        data.extend_from_slice(&2_u16.to_le_bytes());
        data.extend_from_slice(&16_u16.to_le_bytes());
        data.extend_from_slice(b"data");
        data.extend_from_slice(&data_size.to_le_bytes());
        for sample in &self.samples {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        Ok(data)
    }

    fn duration_ms(&self) -> u64 {
        self.samples.len() as u64 * 1_000 / self.sample_rate as u64
    }
}
