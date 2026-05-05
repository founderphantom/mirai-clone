import type {
  GenerationProvider,
  GenerationSubmission,
  ProviderOutput,
  ProviderPollResult,
  ProviderSubmitResult
} from "./provider";
import { dimensionsForQuality } from "./aspect-ratio";
import type { Env } from "../env";

const CLERK_BASE = "https://clerk.higgsfield.ai";
const API_BASE = "https://fnf.higgsfield.ai";
const CLERK_PARAMS = "__clerk_api_version=2025-11-10&_clerk_js_version=5.125.10";

export class HiggsfieldProvider implements GenerationProvider {
  readonly id = "higgsfield";

  constructor(
    private readonly env: Env,
    private readonly media: R2Bucket
  ) {}

  async submit(input: GenerationSubmission): Promise<ProviderSubmitResult> {
    if (input.mode !== "image") {
      throw new Error("HiggsfieldProvider currently supports image jobs only.");
    }

    const jwt = await this.getJwt();
    const uploaded = await this.uploadInput(jwt, input);
    const { width, height } = dimensionsForQuality(input.quality);
    const payload = {
      params: {
        is_custom: false,
        model: "soul_v2",
        prompt: input.prompt,
        style_id: input.cloneConfig.styleId || this.env.HIGGSFIELD_DEFAULT_STYLE_ID,
        style_strength: input.cloneConfig.styleStrength ?? 1,
        custom_reference_id:
          input.cloneConfig.customReferenceId || this.env.HIGGSFIELD_DEFAULT_CHARACTER_ID,
        custom_reference_strength: 1,
        aspect_ratio: input.aspectRatio,
        quality: input.quality,
        enhance_prompt: false,
        width,
        height,
        batch_size: input.batchSize,
        medias: [
          {
            role: "image",
            data: {
              id: uploaded.mediaId,
              type: "media_input",
              url: uploaded.mediaUrl
            }
          }
        ],
        seed: Math.floor(Math.random() * 999999) + 1,
        use_unlim: false,
        use_green: true,
        use_refiner: false,
        negative_prompt: "",
        lora: null,
        chain_enhancer: null,
        model_version: "fast"
      },
      use_unlim: false
    };

    const response = await fetch(`${API_BASE}/jobs/v2/text2image_soul_v2`, {
      method: "POST",
      headers: this.apiHeaders(jwt, "application/json"),
      body: JSON.stringify(payload)
    });

    if (!response.ok) {
      throw new Error(`Higgsfield generation failed (${response.status}): ${await response.text()}`);
    }

    const body = (await response.json()) as {
      job_sets?: Array<{ jobs?: Array<{ id: string }> }>;
    };
    const providerJobIds = body.job_sets?.[0]?.jobs?.map((job) => job.id).filter(Boolean) ?? [];
    if (providerJobIds.length === 0) {
      throw new Error("Higgsfield did not return generation job ids.");
    }

    return { providerJobIds, providerPayload: payload };
  }

  async poll(providerJobIds: string[]): Promise<ProviderPollResult> {
    const jwt = await this.getJwt();
    const pending: string[] = [];

    for (const jobId of providerJobIds) {
      const response = await fetch(`${API_BASE}/jobs/${jobId}/status`, {
        headers: this.apiHeaders(jwt)
      });
      if (!response.ok) {
        return { status: "failed", errorMessage: `Higgsfield status failed for ${jobId}.` };
      }

      const statusBody = (await response.json()) as { status?: string };
      if (statusBody.status === "failed" || statusBody.status === "error") {
        return { status: "failed", errorMessage: `Higgsfield job ${jobId} failed.` };
      }
      if (statusBody.status !== "completed") {
        pending.push(jobId);
      }
    }

    if (pending.length > 0) {
      return { status: "processing", providerJobIds };
    }

    const outputs = await this.getOutputs(jwt, providerJobIds);
    return { status: "completed", outputs };
  }

  private async uploadInput(jwt: string, input: GenerationSubmission) {
    const bytes = await this.readInputBytes(input.inputAsset);
    const contentType = input.inputAsset.contentType || "image/jpeg";
    const slotResponse = await fetch(`${API_BASE}/media/batch`, {
      method: "POST",
      headers: this.apiHeaders(jwt, "application/json"),
      body: JSON.stringify({
        mimetypes: [contentType],
        source: "user_upload",
        force_ip_check: false
      })
    });

    if (!slotResponse.ok) {
      throw new Error(`Higgsfield media slot failed (${slotResponse.status}).`);
    }

    const [slot] = (await slotResponse.json()) as Array<{
      id: string;
      url: string;
      upload_url: string;
    }>;

    const uploadResponse = await fetch(slot.upload_url, {
      method: "PUT",
      headers: { "content-type": contentType },
      body: bytes
    });
    if (!uploadResponse.ok) {
      throw new Error(`Higgsfield presigned upload failed (${uploadResponse.status}).`);
    }

    const confirmResponse = await fetch(`${API_BASE}/media/${slot.id}/upload`, {
      method: "POST",
      headers: this.apiHeaders(jwt, "application/json")
    });
    if (!confirmResponse.ok) {
      throw new Error(`Higgsfield upload confirm failed (${confirmResponse.status}).`);
    }

    return { mediaId: slot.id, mediaUrl: slot.url };
  }

  private async readInputBytes(asset: GenerationSubmission["inputAsset"]): Promise<ArrayBuffer> {
    if (asset.storageKey) {
      const object = await this.media.get(asset.storageKey);
      if (!object) throw new Error("Input asset no longer exists in R2.");
      return await object.arrayBuffer();
    }

    if (asset.remoteUrl) {
      const response = await fetch(asset.remoteUrl);
      if (!response.ok) throw new Error("Could not fetch remote inspiration image.");
      return await response.arrayBuffer();
    }

    throw new Error("Input asset has no storage key or remote URL.");
  }

  private async getOutputs(jwt: string, providerJobIds: string[]): Promise<ProviderOutput[]> {
    const assetsResponse = await fetch(`${API_BASE}/assets?size=1001&category=image`, {
      headers: this.apiHeaders(jwt)
    });
    if (!assetsResponse.ok) {
      throw new Error(`Higgsfield assets fetch failed (${assetsResponse.status}).`);
    }

    const target = new Set(providerJobIds);
    const assets = ((await assetsResponse.json()) as { items?: Array<Record<string, string>> }).items ?? [];
    const outputs: ProviderOutput[] = [];
    for (const asset of assets) {
      const id = asset.id;
      const rawUrl = asset.raw_url;
      if (target.has(id) && rawUrl) {
        outputs.push({
          providerAssetId: id,
          rawUrl,
          shareUrl: await this.enableShare(jwt, id),
          contentType: "image/png"
        });
      }
    }
    return providerJobIds.flatMap((id) => outputs.filter((output) => output.providerAssetId === id));
  }

  private async enableShare(jwt: string, jobId: string): Promise<string | undefined> {
    await fetch(`${API_BASE}/sharing-configs?asset_id=${jobId}`, {
      headers: this.apiHeaders(jwt)
    });

    const response = await fetch(`${API_BASE}/sharing-configs?asset_id=${jobId}`, {
      method: "PATCH",
      headers: this.apiHeaders(jwt, "application/json"),
      body: JSON.stringify({
        link_access_level: "edit",
        redirect_url: `https://higgsfield.ai/share/${jobId}?utm_source=copylink&utm_medium=share&utm_campaign=asset_share&utm_content=image`
      })
    });

    if (!response.ok) return undefined;
    const body = (await response.json()) as { url?: string };
    return body.url;
  }

  private async getJwt(): Promise<string> {
    if (this.env.HIGGSFIELD_JWT) return this.env.HIGGSFIELD_JWT;
    if (!this.env.HIGGSFIELD_SESSION_ID || !this.env.HIGGSFIELD_CLIENT_COOKIE) {
      throw new Error(
        "Configure HIGGSFIELD_JWT or HIGGSFIELD_SESSION_ID/HIGGSFIELD_CLIENT_COOKIE for the temporary Higgsfield backend."
      );
    }

    const response = await fetch(
      `${CLERK_BASE}/v1/client/sessions/${this.env.HIGGSFIELD_SESSION_ID}/tokens?debug=skip_cache&${CLERK_PARAMS}`,
      {
        method: "POST",
        headers: {
          "content-type": "application/x-www-form-urlencoded",
          cookie: `__client=${this.env.HIGGSFIELD_CLIENT_COOKIE}`
        },
        body: "organization_id="
      }
    );

    if (!response.ok) {
      throw new Error(`Higgsfield Clerk token refresh failed (${response.status}).`);
    }

    const body = (await response.json()) as { jwt?: string };
    if (!body.jwt) throw new Error("Higgsfield Clerk token response did not include a JWT.");
    return body.jwt;
  }

  private apiHeaders(jwt: string, contentType?: string): HeadersInit {
    return {
      ...(contentType ? { "content-type": contentType } : {}),
      accept: "*/*",
      authorization: `Bearer ${jwt}`,
      origin: "https://higgsfield.ai",
      "user-agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:150.0) Gecko/20100101 Firefox/150.0"
    };
  }
}
