import { Bell, CheckCircle2 } from "lucide-react";
import type { Job } from "../types";

export function InboxScreen({ jobs }: { jobs: Job[] }) {
  const active = jobs.filter((job) => ["queued", "processing"].includes(job.status));
  return (
    <div className="inbox-list">
      {active.length === 0 && (
        <article className="inbox-row">
          <CheckCircle2 size={22} />
          <div>
            <h2>All clear</h2>
            <p>No active generations</p>
          </div>
        </article>
      )}
      {active.map((job) => (
        <article className="inbox-row" key={job.id}>
          <Bell size={22} />
          <div>
            <h2>{job.clone_name || "Generation"}</h2>
            <p>{job.status}</p>
          </div>
        </article>
      ))}
    </div>
  );
}
