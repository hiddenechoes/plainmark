// React context carrying what a previewed ` ```query ` block needs to run and
// act: a `run` that executes the block against the live index for today's local
// date, a navigate callback (to open a result's source note at its line), and a
// toggle callback (to flip a result's checkbox in its source file). Kept in its
// own module so the component file exports only a component (Fast Refresh).
import { createContext } from "react";
import type { QueryResponse } from "../tauri";

/** Arguments for toggling a result's checkbox — the path/line locate it and the
 * text/done state let the backend re-verify the target before editing (§7.1). */
export interface ToggleArgs {
  path: string;
  line: number;
  text: string;
  done: boolean;
}

export interface QueryContextValue {
  /** Run a query block's source against the index (resolves `today` locally). */
  run: (source: string) => Promise<QueryResponse>;
  /** Open a result's source note (absolute path) at its 1-based line. */
  onNavigate: (path: string, line: number) => void;
  /** Toggle a result's checkbox; resolves to the new done-state, or rejects. */
  onToggle: (args: ToggleArgs) => Promise<boolean>;
}

export const QueryContext = createContext<QueryContextValue | null>(null);
