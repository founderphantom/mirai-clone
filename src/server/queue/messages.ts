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
