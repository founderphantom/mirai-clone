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
  name: string;
  handle: string;
  persona: string;
  style_prompt: string;
  reference_count?: number;
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
  prompt: string;
  updated_at: string;
  output_count?: number;
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
  async function createClone(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    const form = new FormData(event.currentTarget);
    await api("/api/clones", {
      method: "POST",
      body: JSON.stringify({
        name: form.get("name"),
        handle: form.get("handle"),
        persona: form.get("persona"),
        stylePrompt: form.get("stylePrompt")
      })
    });
    event.currentTarget.reset();
    setBusy(false);
    await reload();
  }

  return (
    <div className="split">
      <form className="panel form-grid" onSubmit={createClone}>
        <h2>New Clone</h2>
        <input name="name" placeholder="Name" required />
        <input name="handle" placeholder="Handle" />
        <textarea name="persona" placeholder="Identity" rows={5} />
        <textarea name="stylePrompt" placeholder="Visual style" rows={5} />
        <button className="primary" disabled={busy}>Create clone</button>
      </form>
      <div className="grid-list">
        {clones.map((clone) => (
          <article className="item-card" key={clone.id}>
            <h3>{clone.name}</h3>
            <p>@{clone.handle}</p>
            <footer>{clone.reference_count || 0} refs · {clone.generation_count || 0} jobs</footer>
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
            <img src={item.image_url || item.thumbnail_url || ""} alt={item.title || item.platform} />
            <span>{item.author_handle || item.platform}</span>
          </button>
        ))}
      </div>
    </div>
  );
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
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!clone || !inspiration) return;
    setBusy(true);
    const form = new FormData(event.currentTarget);
    await api("/api/generations", {
      method: "POST",
      body: JSON.stringify({
        cloneId: clone.id,
        prompt: form.get("prompt"),
        inspirationAssetId: inspiration.type === "asset" ? inspiration.id : undefined,
        discoveryItemId: inspiration.type === "discovery" ? inspiration.id : undefined,
        quality: form.get("quality"),
        batchSize: Number(form.get("batchSize") || 4)
      })
    });
    setBusy(false);
    await onSubmitted();
  }

  return (
    <div className="split">
      <form className="panel form-grid" onSubmit={submit}>
        <h2>{clone?.name || "Select a clone"}</h2>
        <textarea name="prompt" placeholder="Prompt" rows={6} />
        <select name="quality" defaultValue="1080p">
          <option value="1080p">1080p</option>
          <option value="2K">2K</option>
        </select>
        <input name="batchSize" type="number" min={1} max={4} defaultValue={4} />
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
  return (
    <select value={selected} onChange={(event) => onSelect(event.target.value)}>
      {clones.map((clone) => <option key={clone.id} value={clone.id}>{clone.name}</option>)}
    </select>
  );
}

function FullScreenLoading() {
  return <main className="auth-screen"><Loader2 className="spin" /></main>;
}

function viewTitle(view: string) {
  return views.find(([id]) => id === view)?.[2] || "Mirai";
}
