import type { Job } from "../types";

export function LibraryScreen({ jobs, onRefresh }: { jobs: Job[]; onRefresh: () => Promise<void> }) {
  return (
    <div className="library-grid">
      {jobs.map((job) => (
        <article className="library-item" key={job.id}>
          {job.preview_media_id ? <img src={`/api/media/${job.preview_media_id}`} alt={job.clone_name || "Generation"} /> : <div className="image-placeholder" />}
          <div>
            <h2>{job.clone_name || "Generation"}</h2>
            <p>{job.status}</p>
          </div>
        </article>
      ))}
      <button className="library-refresh" onClick={onRefresh}>Refresh</button>
    </div>
  );
}
