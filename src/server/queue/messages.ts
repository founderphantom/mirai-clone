export type GenerationQueueMessage =
  | {
      type: "submit_generation";
      jobId: string;
      userId: string;
      attempt?: number;
    }
  | {
      type: "poll_generation";
      jobId: string;
      userId: string;
      providerJobIds: string[];
      attempt?: number;
    };

export type OnboardingQueueMessage =
  | {
      type: "run_instagram_harvest";
      jobId: string;
      userId: string;
    }
  | {
      type: "analyze_persona";
      cloneId: string;
      userId: string;
    }
  | {
      type: "seed_inspiration_pool";
      cloneId: string;
      userId: string;
      bubbleIds?: string[];
    };

export type AppQueueMessage = GenerationQueueMessage | OnboardingQueueMessage;

export function isOnboardingQueueMessage(message: AppQueueMessage): message is OnboardingQueueMessage {
  return (
    message.type === "run_instagram_harvest" ||
    message.type === "analyze_persona" ||
    message.type === "seed_inspiration_pool"
  );
}
