// Backlinks / linked-mentions panel (SPEC §8.2). Lists every note that links to
// the active note, with a context snippet; clicking a mention navigates to it.
// Refreshes when the note changes and whenever the index updates (live), so a
// new or removed inbound link shows up without a restart.
import { useEffect, useState } from "react";
import { getBacklinks, onIndexUpdated, type Backlink } from "../lib/tauri";

interface BacklinksListProps {
  backlinks: Backlink[];
  onNavigate: (path: string) => void;
}

/** Pure presentational list (no data fetching), so it's easy to render-test. */
export function BacklinksList({ backlinks, onNavigate }: BacklinksListProps) {
  if (backlinks.length === 0) {
    return <p className="panel-empty">No backlinks.</p>;
  }
  return (
    <ul className="backlinks-list">
      {backlinks.map((b, i) => (
        <li key={`${b.from}:${b.line}:${i}`}>
          <button type="button" className="backlink-item" onClick={() => onNavigate(b.from)}>
            <span className="backlink-source">
              {b.fromTitle}
              <span className="backlink-line">:{b.line}</span>
            </span>
            <span className="backlink-snippet">{b.snippet}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

interface BacklinksPanelProps {
  /** Absolute path of the active note, or `null` when none is open. */
  notePath: string | null;
  onNavigate: (path: string) => void;
}

export function BacklinksPanel({ notePath, onNavigate }: BacklinksPanelProps) {
  const [backlinks, setBacklinks] = useState<Backlink[]>([]);

  useEffect(() => {
    let active = true;
    // Resolve to [] for "no note" so the only setState happens asynchronously
    // (keeps this out of the synchronous-setState-in-effect rule).
    const load = () => {
      (notePath ? getBacklinks(notePath) : Promise.resolve<Backlink[]>([]))
        .then((b) => {
          if (active) setBacklinks(b);
        })
        .catch(() => {});
    };
    load();
    const unlisten = onIndexUpdated(load);
    return () => {
      active = false;
      void unlisten.then((off) => off());
    };
  }, [notePath]);

  return (
    <div className="backlinks-panel">
      <h2 className="panel-title">
        Backlinks
        {backlinks.length > 0 && <span className="panel-count">{backlinks.length}</span>}
      </h2>
      {notePath ? (
        <BacklinksList backlinks={backlinks} onNavigate={onNavigate} />
      ) : (
        <p className="panel-empty">No note open.</p>
      )}
    </div>
  );
}
