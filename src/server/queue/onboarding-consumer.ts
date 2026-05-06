import type { Env } from "../env";
import { runInstagramHarvestJob } from "../services/instagram-harvest";
import { seedInspirationPool } from "../services/inspiration-pool";
import { ensurePersonaBubblesForClone } from "../services/persona-agent";
import type { OnboardingQueueMessage } from "./messages";

export async function handleOnboardingBatch(
  batch: MessageBatch<OnboardingQueueMessage>,
  env: Env
) {
  for (const message of batch.messages) {
    try {
      await handleMessage(message.body, env);
      message.ack();
    } catch (error) {
      console.error("onboarding queue failed", error);
      message.retry({ delaySeconds: 30 });
    }
  }
}

async function handleMessage(message: OnboardingQueueMessage, env: Env) {
  if (message.type === "run_instagram_harvest") {
    const job = await runInstagramHarvestJob(env, message.jobId, message.userId);
    if (job.clone_id) {
      await ensurePersonaBubblesForClone(env, message.userId, job.clone_id);
    }
    return;
  }

  if (message.type === "analyze_persona") {
    await ensurePersonaBubblesForClone(env, message.userId, message.cloneId);
    return;
  }

  if (message.type === "seed_inspiration_pool") {
    await seedInspirationPool(env, message.userId, message.cloneId, message.bubbleIds);
  }
}
