use worker::{Bucket, D1Database, Env, Queue, Result as WorkerResult};

pub struct Bindings {
    pub db: D1Database,
    pub media: Bucket,
    pub clone_training_queue: Queue,
    pub generation_queue: Queue,
    pub niche_research_queue: Queue,
}

impl Bindings {
    pub fn from_env(env: &Env) -> WorkerResult<Self> {
        Ok(Self {
            db: env.d1("DB")?,
            media: env.bucket("MEDIA")?,
            clone_training_queue: env.queue("CLONE_TRAINING_QUEUE")?,
            generation_queue: env.queue("GENERATION_QUEUE")?,
            niche_research_queue: env.queue("NICHE_RESEARCH_QUEUE")?,
        })
    }
}
