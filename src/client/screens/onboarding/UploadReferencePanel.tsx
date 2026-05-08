import { Camera, CheckCircle2, CloudUpload, Image as ImageIcon, LockKeyhole, Sparkles, UserRound } from "lucide-react";
import { type FormEvent, useMemo, useState } from "react";
import {
  MAX_REFERENCE_PHOTOS,
  REFERENCE_EXAMPLES,
  REFERENCE_GUIDANCE,
  validateReferenceFiles
} from "./upload-reference-guidance";

type UploadReferencePanelProps = {
  busy: boolean;
  onSubmit: (form: FormData) => Promise<void>;
};

export function UploadReferencePanel({ busy, onSubmit }: UploadReferencePanelProps) {
  const [name, setName] = useState("My Soul");
  const [files, setFiles] = useState<File[]>([]);
  const [touched, setTouched] = useState(false);
  const validation = useMemo(() => validateReferenceFiles(files), [files]);
  const selectedSummary = fileSummary(files);
  const showValidation = touched || files.length > 0;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setTouched(true);
    if (!validation.valid) return;

    const form = new FormData();
    form.set("name", name.trim() || "My Soul");
    for (const file of files) form.append("photos", file);
    await onSubmit(form);
  }

  return (
    <section className="upload-reference-card" aria-labelledby="upload-reference-title">
      <div className="upload-reference-main">
        <div className="upload-reference-heading">
          <span className="upload-reference-icon" aria-hidden="true">
            <Camera size={22} />
          </span>
          <div>
            <span className="app-kicker">Creator setup</span>
            <h2 id="upload-reference-title">Upload reference photos</h2>
            <p>Share 5-15 clear images that capture your look, style, and content vibe.</p>
          </div>
        </div>

        <form className="upload-reference-form upload-reference-studio" onSubmit={submit}>
          <label className="upload-reference-field">
            <span><UserRound size={18} /> Soul name</span>
            <input
              name="name"
              onChange={(event) => setName(event.target.value)}
              placeholder="My Soul"
              value={name}
            />
          </label>

          <div className={validation.valid ? "upload-reference-dropzone ready" : "upload-reference-dropzone"}>
            <input
              aria-describedby="upload-reference-status"
              aria-label="Choose 5 to 15 reference photos"
              disabled={busy}
              multiple
              name="photos"
              onChange={(event) => {
                setTouched(true);
                setFiles(Array.from(event.target.files ?? []));
              }}
              type="file"
              accept="image/*"
            />
            <span className="upload-reference-upload-icon" aria-hidden="true">
              <CloudUpload size={30} />
            </span>
            <strong>{files.length > 0 ? `${files.length}/${MAX_REFERENCE_PHOTOS} photos selected` : "Tap to upload photos"}</strong>
            <small>JPG, PNG, WebP or HEIC. Max 15 MB each.</small>
            <span className="upload-reference-choose">Choose files</span>
          </div>

          <div
            className={validation.valid ? "upload-reference-status ready" : "upload-reference-status"}
            id="upload-reference-status"
            role={showValidation && !validation.valid ? "alert" : undefined}
          >
            <span aria-hidden="true">{validation.valid ? <CheckCircle2 size={16} /> : <ImageIcon size={16} />}</span>
            <div>
              <strong>{showValidation ? validation.message : "Select 5-15 clear reference photos."}</strong>
              <small>{selectedSummary}</small>
            </div>
          </div>

          <button className="primary upload-reference-submit" disabled={busy || !validation.valid}>
            {busy ? <Sparkles className="spin" size={16} /> : <Camera size={16} />}
            Create from uploads
          </button>
        </form>
      </div>

      <aside className="upload-reference-guide" aria-label="Reference photo guidance">
        <div className="upload-reference-guide-top">
          <span className="upload-reference-icon" aria-hidden="true">
            <LockKeyhole size={22} />
          </span>
          <div>
            <h3>Reference quality checklist</h3>
            <p>Good references help Mirai recognize you across fashion, beauty, travel, and content scenes.</p>
          </div>
        </div>

        <div className="upload-reference-checklist">
          {REFERENCE_GUIDANCE.map((item) => (
            <div key={item}>
              <CheckCircle2 size={17} />
              <span>{item}</span>
            </div>
          ))}
        </div>

        <div className="upload-reference-examples" aria-label="Example reference styles">
          {REFERENCE_EXAMPLES.map((example) => (
            <figure key={example.label}>
              <img src={example.src} alt={`${example.label} reference example`} />
              <figcaption>
                <strong>{example.label}</strong>
                <span>{example.caption}</span>
              </figcaption>
            </figure>
          ))}
        </div>

        <div className="upload-reference-privacy">
          <LockKeyhole size={18} />
          <span>Your photos stay private and are used only to build your identity reference.</span>
        </div>
      </aside>
    </section>
  );
}

function fileSummary(files: File[]) {
  if (files.length === 0) return "No files selected yet.";
  const names = files.slice(0, 3).map((file) => file.name).join(", ");
  const suffix = files.length > 3 ? ` and ${files.length - 3} more` : "";
  return `${names}${suffix}`;
}
