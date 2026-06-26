// Renders a fenced ` ```query ` block (SPEC §8.5): runs the block against the
// live task index, shows a filtered/sorted/limited checklist, and keeps it live
// by re-running on `index://updated`. Each result links to its source file+line
// and its checkbox writes `[ ]`↔`[x]` back through the safe Rust path.
import { useCallback, useContext, useEffect, useState } from "react";
import { QueryContext } from "../lib/query/context";
import {
  CHANGED_ON_DISK,
  onIndexUpdated,
  TASK_MISMATCH,
  type QueryResponse,
  type TaskResult,
} from "../lib/tauri";

/** Turn a backend toggle error into a short, human message. */
function toggleMessage(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  if (text.includes(TASK_MISMATCH)) {
    return "That task changed on disk — results refreshed; try again.";
  }
  if (text.includes(CHANGED_ON_DISK)) {
    return "The note changed on disk — results refreshed; try again.";
  }
  return text;
}

export function QueryBlock({ source }: { source: string }) {
  const ctx = useContext(QueryContext);
  const [response, setResponse] = useState<QueryResponse | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [toggleError, setToggleError] = useState<string | null>(null);

  // Run on mount, when the block's source changes, and whenever the index
  // updates (an edit, a rename, or our own write-back) so results stay live.
  useEffect(() => {
    if (!ctx) return;
    let active = true;
    const run = () => {
      ctx
        .run(source)
        .then((r) => {
          if (active) {
            setResponse(r);
            setFailure(null);
          }
        })
        .catch((e) => {
          if (active) setFailure(String(e));
        });
    };
    run();
    const unlisten = onIndexUpdated(run);
    return () => {
      active = false;
      void unlisten.then((off) => off());
    };
  }, [ctx, source]);

  const handleToggle = useCallback(
    (task: TaskResult) => {
      if (!ctx) return;
      setToggleError(null);
      // On success the backend emits `index://updated`, which re-runs the query
      // above and flips the box; on failure we surface why (and results refresh).
      ctx
        .onToggle({ path: task.path, line: task.line, text: task.text, done: task.done })
        .catch((e) => setToggleError(toggleMessage(e)));
    },
    [ctx],
  );

  if (!ctx) return null;

  if (failure) {
    return <div className="query-block query-error">Query failed: {failure}</div>;
  }
  // A malformed directive is reported inline, never thrown (SPEC §8.5).
  if (response?.error) {
    return (
      <div className="query-block query-error">
        <span className="query-error-label">Invalid query:</span> {response.error}
      </div>
    );
  }

  const tasks = response?.tasks ?? [];
  return (
    <div className="query-block">
      {toggleError && <div className="query-toggle-error">{toggleError}</div>}
      {tasks.length === 0 ? (
        <p className="query-empty">No matching tasks.</p>
      ) : (
        <ul className="query-results">
          {tasks.map((task, i) => (
            <li key={`${task.path}:${task.line}:${i}`} className="query-result">
              <input
                type="checkbox"
                className="query-check"
                checked={task.done}
                onChange={() => handleToggle(task)}
                aria-label={task.done ? "Mark task not done" : "Mark task done"}
              />
              <span className={task.done ? "query-text query-text-done" : "query-text"}>
                {task.text}
              </span>
              <button
                type="button"
                className="query-source"
                title={`Open ${task.relPath} at line ${task.line}`}
                onClick={() => ctx.onNavigate(task.path, task.line)}
              >
                {task.relPath}:{task.line}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
