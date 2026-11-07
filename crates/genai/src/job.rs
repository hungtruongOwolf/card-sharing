//! The `genai.effect.run` job handler (ADR-0006 queue): one effect per job, retried with backoff
//! while the provider or a gate is unavailable; the last attempt fails the job and refunds it.

use std::sync::Arc;

use media::MediaService;
use platform::jobs::{ClaimedJob, JobError, JobFuture, JobHandler};
use uuid::Uuid;

use crate::moderation::Gates;
use crate::provider::EffectProvider;
use crate::service::{GenAiService, RunOutcome};

pub struct RunEffectJob {
    pub service: Arc<GenAiService>,
    pub media: Arc<MediaService>,
    pub provider: Arc<dyn EffectProvider>,
    pub gates: Arc<Gates>,
}

impl JobHandler for RunEffectJob {
    fn handle(&self, job: ClaimedJob) -> JobFuture {
        let service = Arc::clone(&self.service);
        let media = Arc::clone(&self.media);
        let provider = Arc::clone(&self.provider);
        let gates = Arc::clone(&self.gates);
        Box::pin(async move {
            let Some(id) = job
                .payload
                .get("job_id")
                .and_then(|v| v.as_str())
                .and_then(|v| Uuid::try_parse(v).ok())
            else {
                return Err(JobError::Permanent("payload without job_id".to_owned()));
            };
            let last_attempt = job.attempt >= job.max_attempts;
            match service
                .run(id, &media, provider.as_ref(), &gates, last_attempt)
                .await
            {
                Ok(RunOutcome::Retry) => {
                    Err(JobError::Retry("provider or gate unavailable".to_owned()))
                }
                Ok(_) => Ok(()),
                Err(err) => Err(JobError::Retry(err.to_string())),
            }
        })
    }
}
