import {
  Archive,
  CreditCard,
  ImagePlus,
  Images,
  Layers3,
  Loader2,
  LogOut,
  RefreshCcw,
  Sparkles,
  UserRound
} from "lucide-react";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { api } from "./lib/api";
import { authClient } from "./lib/auth-client";

type Clone = {
  id: string;
  display_name: string;
  handle: string;
  source?: "manual_upload" | "starter" | "future_instagram";
  status?: "active" | "archived" | "deleting";
  soul_status?: "draft" | "queued" | "training" | "ready" | "failed" | "provider_action_required";
  provider?: "higgsfield";
  provider_soul_id?: string | null;
  reference_count_total?: number;
  reference_count_training_selected?: number;
  generation_count?: number;
};

type DiscoveryItem = {
  id: string;
  title: string;
  platform: string;
  image_url: string | null;
  thumbnail_url: string | null;
  source_url: string | null;
  author_handle: string;
};

type Job = {
  id: string;
  clone_id: string;
  clone_name?: string;
  status: string;
  prompt?: string | null;
  updated_at: string;
  output_count?: number;
  preview_media_id?: string | null;
};

type Account = {
  user: { id: string; name?: string; email?: string };
  billing: { checkoutEnabled: boolean; portalEnabled: boolean; server: string };
};

type Inspiration = { type: "discovery"; id: string; imageUrl: string } | { type: "asset"; id: string; imageUrl: string };

const views = [
  ["clones", Layers3, "Clones"],
  ["discovery", Images, "Discovery"],
  ["generate", Sparkles, "Generate"],
  ["history", Archive, "History"],
  ["account", UserRound, "Account"]
] as const;

export function App() {
  const [account, setAccount] = useState<Account | null>(null);
  const [authLoading, setAuthLoading] = useState(true);
  const [view, setView] = useState<(typeof views)[number][0]>("clones");
  const [clones, setClones] = useState<Clone[]>([]);
  const [selectedCloneId, setSelectedCloneId] = useState("");
  const [inspiration, setInspiration] = useState<Inspiration | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const selectedClone = useMemo(
    () => clones.find((clone) => clone.id === selectedCloneId) || clones[0],
    [clones, selectedCloneId]
  );

  async function loadAll() {
    const acct = await api<Account>("/api/account");
    const cloneData = await api<{ clones: Clone[] }>("/api/clones");
    const jobData = await api<{ jobs: Job[] }>("/api/generations");
    setAccount(acct);
    setClones(cloneData.clones);
    setJobs(jobData.jobs);
    setSelectedCloneId((current) => current || cloneData.clones[0]?.id || "");
  }

  useEffect(() => {
    loadAll()
      .catch(() => setAccount(null))
      .finally(() => setAuthLoading(false));
  }, []);

  if (authLoading) return <FullScreenLoading />;
  if (!account) return <AuthPanel onDone={() => loadAll().then(() => setAuthLoading(false))} />;

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand-mark">Mirai</div>
        <nav>
          {views.map(([id, Icon, label]) => (
            <button className={view === id ? "active" : ""} key={id} onClick={() => setView(id)}>
              <Icon size={18} />
              <span>{label}</span>
            </button>
          ))}
        </nav>
        <button
          className="ghost"
          onClick={async () => {
            await authClient.signOut();
            setAccount(null);
          }}
        >
          <LogOut size={18} />
          <span>Sign out</span>
        </button>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div>
            <p>{account.user.email}</p>
            <h1>{viewTitle(view)}</h1>
          </div>
          <ClonePicker clones={clones} selected={selectedClone?.id || ""} onSelect={setSelectedCloneId} />
        </header>

        {view === "clones" && <ClonesPage clones={clones} reload={loadAll} />}
        {view === "discovery" && (
          <DiscoveryPage
            selectedClone={selectedClone}
            onChoose={(item) => {
              setInspiration({ type: "discovery", id: item.id, imageUrl: item.image_url || item.thumbnail_url || "" });
              setView("generate");
            }}
            onUpload={(asset) => {
              setInspiration(asset);
              setView("generate");
            }}
          />
        )}
        {view === "generate" && (
          <GeneratePage clone={selectedClone} inspiration={inspiration} onSubmitted={loadAll} />
        )}
        {view === "history" && <HistoryPage jobs={jobs} reload={loadAll} />}
        {view === "account" && <AccountPage account={account} />}
      </section>
    </main>
  );
}

function AuthPanel({ onDone }: { onDone: () => void }) {
  const [mode, setMode] = useState<"signin" | "signup">("signin");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError("");
    const form = new FormData(event.currentTarget);
    const email = String(form.get("email"));
    const password = String(form.get("password"));
    const name = String(form.get("name") || "Mirai creator");
    const result =
      mode === "signin"
        ? await authClient.signIn.email({ email, password })
        : await authClient.signUp.email({ email, password, name });
    setBusy(false);
    if (result.error) setError(result.error.message || "Authentication failed.");
    else onDone();
  }

  return (
    <main className="auth-screen">
      <section className="auth-panel">
        <div className="brand-mark">Mirai</div>
        <div className="segmented">
          <button className={mode === "signin" ? "active" : ""} onClick={() => setMode("signin")}>Sign in</button>
          <button className={mode === "signup" ? "active" : ""} onClick={() => setMode("signup")}>Sign up</button>
        </div>
        <form onSubmit={submit}>
          {mode === "signup" && <input name="name" placeholder="Name" />}
          <input name="email" type="email" placeholder="Email" required />
          <input name="password" type="password" placeholder="Password" minLength={8} required />
          {error && <p className="error">{error}</p>}
          <button className="primary" disabled={busy}>
            {busy && <Loader2 className="spin" size={16} />}
            {mode === "signin" ? "Sign in" : "Create account"}
          </button>
        </form>
      </section>
    </main>
  );
}

function ClonesPage({ clones, reload }: { clones: Clone[]; reload: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function createClone(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    setBusy(true);
    setError("");
    try {
      const form = new FormData(formElement);
      await api("/api/clones/manual-upload", {
        method: "POST",
        body: form
      });
      formElement.reset();
      await reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not create clone.");
      await reload().catch(() => undefined);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="split">
      <form className="panel form-grid" onSubmit={createClone}>
        <h2>New Clone</h2>
        <input name="displayName" placeholder="Soul name" required />
        <input name="photos" type="file" accept="image/*" multiple required />
        {error && <p className="error">{error}</p>}
        <button className="primary" disabled={busy}>Create clone</button>
      </form>
      <div className="grid-list">
        {clones.map((clone) => (
          <article className="item-card" key={clone.id}>
            <h3>{cloneDisplayName(clone)}</h3>
            <p>@{clone.handle}</p>
            <footer>{clone.reference_count_total || 0} refs · {clone.generation_count || 0} jobs</footer>
          </article>
        ))}
      </div>
    </div>
  );
}

function DiscoveryPage({
  selectedClone,
  onChoose,
  onUpload
}: {
  selectedClone?: Clone;
  onChoose: (item: DiscoveryItem) => void;
  onUpload: (asset: Inspiration) => void;
}) {
  const [source, setSource] = useState("youtube-shorts");
  const [items, setItems] = useState<DiscoveryItem[]>([]);
  const [busy, setBusy] = useState(false);

  async function load(force = false) {
    setBusy(true);
    const data = force
      ? await api<{ items: DiscoveryItem[] }>("/api/discovery/refresh", {
          method: "POST",
          body: JSON.stringify({ source })
        })
      : await api<{ items: DiscoveryItem[] }>(`/api/discovery/feed?source=${source}`);
    setItems(data.items);
    setBusy(false);
  }

  useEffect(() => {
    load().catch(() => setItems([]));
  }, [source]);

  async function upload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    if (selectedClone?.id) form.set("cloneId", selectedClone.id);
    const result = await api<{ media: { id: string } }>("/api/media/upload", { method: "POST", body: form });
    onUpload({ type: "asset", id: result.media.id, imageUrl: `/api/media/${result.media.id}` });
  }

  return (
    <div className="stack">
      <div className="toolbar">
        <select value={source} onChange={(event) => setSource(event.target.value)}>
          <option value="youtube-shorts">YouTube Shorts</option>
          <option value="tiktok-trending">TikTok Trending</option>
          <option value="instagram-reels">Instagram Reels</option>
        </select>
        <button onClick={() => load(true)} disabled={busy}><RefreshCcw size={16} />Refresh</button>
        <form className="upload-inline" onSubmit={upload}>
          <input name="file" type="file" accept="image/*" required />
          <button><ImagePlus size={16} />Upload</button>
        </form>
      </div>
      <div className="masonry">
        {items.map((item) => (
          <button className="image-tile" key={item.id} onClick={() => onChoose(item)}>
            <DiscoveryTileImage item={item} />
            <span>{item.author_handle || item.platform}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function DiscoveryTileImage({ item }: { item: DiscoveryItem }) {
  const primary = item.image_url || item.thumbnail_url || "";
  const [src, setSrc] = useState(primary);

  useEffect(() => {
    setSrc(primary);
  }, [primary]);

  if (!src) return <div className="image-placeholder" aria-hidden="true" />;

  return (
    <img
      src={src}
      alt={item.title || item.platform}
      onError={() => setSrc(fallbackDiscoveryImageSrc(src, item) || "")}
    />
  );
}

function fallbackDiscoveryImageSrc(current: string, item: DiscoveryItem): string | null {
  const candidates = [item.thumbnail_url, youtubeThumbnailFallback(current)];
  return candidates.find((candidate) => candidate && candidate !== current) || null;
}

function youtubeThumbnailFallback(value: string): string | null {
  try {
    const url = new URL(value);
    if (!url.hostname.endsWith("img.youtube.com") || !url.pathname.endsWith("/maxresdefault.jpg")) {
      return null;
    }
    url.pathname = url.pathname.replace(/\/maxresdefault\.jpg$/, "/hqdefault.jpg");
    return url.toString();
  } catch {
    return null;
  }
}

function GeneratePage({
  clone,
  inspiration,
  onSubmitted
}: {
  clone?: Clone;
  inspiration: Inspiration | null;
  onSubmitted: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!clone || !inspiration) return;
    setBusy(true);
    setError("");
    setNotice("");
    const form = new FormData(event.currentTarget);
    const prompt = String(form.get("prompt") || "").trim();
    try {
      await api("/api/generations", {
        method: "POST",
        body: JSON.stringify({
          cloneId: clone.id,
          ...(prompt ? { prompt } : {}),
          inspirationAssetId: inspiration.type === "asset" ? inspiration.id : undefined,
          discoveryItemId: inspiration.type === "discovery" ? inspiration.id : undefined,
          quality: form.get("quality"),
          batchSize: Number(form.get("batchSize") || 4)
        })
      });
      setNotice("Generation queued.");
      await onSubmitted();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not queue generation.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="split">
      <form className="panel form-grid" onSubmit={submit}>
        <h2>{clone ? cloneDisplayName(clone) : "Select a clone"}</h2>
        <details className="advanced-field">
          <summary>Prompt override</summary>
          <textarea name="prompt" placeholder="Optional prompt" rows={4} />
        </details>
        <select name="quality" defaultValue="1080p">
          <option value="1080p">1080p</option>
          <option value="2K">2K</option>
        </select>
        <input name="batchSize" type="number" min={1} max={4} defaultValue={4} />
        {error && <p className="error">{error}</p>}
        {notice && <p className="notice">{notice}</p>}
        <button className="primary" disabled={!clone || !inspiration || busy}>
          {busy && <Loader2 className="spin" size={16} />}
          Generate
        </button>
      </form>
      <div className="preview-box">{inspiration && <img src={inspiration.imageUrl} alt="Selected inspiration" />}</div>
    </div>
  );
}

function HistoryPage({ jobs, reload }: { jobs: Job[]; reload: () => Promise<void> }) {
  return (
    <div className="stack">
      <div className="toolbar"><button onClick={reload}><RefreshCcw size={16} />Refresh</button></div>
      <div className="grid-list">
        {jobs.map((job) => (
          <article className="item-card" key={job.id}>
            {job.preview_media_id && (
              <img className="history-preview" src={`/api/media/${job.preview_media_id}`} alt={`${job.clone_name || "Clone"} output`} />
            )}
            <h3>{job.clone_name || job.clone_id}</h3>
            <p>{job.prompt || "No prompt"}</p>
            <footer>{job.status} · {job.output_count || 0} outputs · {new Date(job.updated_at).toLocaleString()}</footer>
          </article>
        ))}
      </div>
    </div>
  );
}

function AccountPage({ account }: { account: Account }) {
  return (
    <div className="panel account-panel">
      <h2>{account.user.name || account.user.email}</h2>
      <p>Polar {account.billing.server}</p>
      <div className="toolbar">
        <button disabled={!account.billing.checkoutEnabled} onClick={() => authClient.checkout({ slug: "pro" })}>
          <CreditCard size={16} />Upgrade
        </button>
        <button disabled={!account.billing.portalEnabled} onClick={() => authClient.customer.portal()}>
          <UserRound size={16} />Portal
        </button>
      </div>
    </div>
  );
}

function ClonePicker({ clones, selected, onSelect }: { clones: Clone[]; selected: string; onSelect: (id: string) => void }) {
  if (clones.length === 0) {
    return (
      <select disabled>
        <option>No clones</option>
      </select>
    );
  }

  return (
    <select value={selected} onChange={(event) => onSelect(event.target.value)}>
      {clones.map((clone) => <option key={clone.id} value={clone.id}>{cloneDisplayName(clone)}</option>)}
    </select>
  );
}

function FullScreenLoading() {
  return <main className="auth-screen"><Loader2 className="spin" /></main>;
}

function viewTitle(view: string) {
  return views.find(([id]) => id === view)?.[2] || "Mirai";
}

function cloneDisplayName(clone: Clone) {
  return clone.display_name || clone.handle || "Mirai Soul";
}
