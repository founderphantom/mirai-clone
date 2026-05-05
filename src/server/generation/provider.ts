export type GenerationMode = "image" | "video";

export type CloneProviderConfig = {
  customReferenceId?: string;
  styleId?: string;
  styleStrength?: number;
};

export type ProviderInputAsset = {
  id: string;
  storageKey: string | null;
  remoteUrl: string | null;
  contentType: string | null;
};

export type GenerationSubmission = {
  jobId: string;
  userId: string;
  cloneId: string;
  prompt: string;
  aspectRatio: string;
  quality: string;
  batchSize: number;
  mode: GenerationMode;
  inputAsset: ProviderInputAsset;
  cloneConfig: CloneProviderConfig;
};

export type ProviderSubmitResult = {
  providerJobIds: string[];
  providerPayload: Record<string, unknown>;
};

export type ProviderOutput = {
  providerAssetId: string;
  rawUrl: string;
  shareUrl?: string;
  contentType?: string;
};

export type ProviderPollResult =
  | { status: "processing"; providerJobIds: string[] }
  | { status: "failed"; errorMessage: string }
  | { status: "completed"; outputs: ProviderOutput[] };

export interface GenerationProvider {
  readonly id: string;
  submit(input: GenerationSubmission): Promise<ProviderSubmitResult>;
  poll(providerJobIds: string[]): Promise<ProviderPollResult>;
}
