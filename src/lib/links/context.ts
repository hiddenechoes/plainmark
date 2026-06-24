// React context carrying everything a previewed `[[wikilink]]` needs to resolve
// and act: the resolver, the source note's vault-relative path, and the navigate
// / create callbacks. Kept in its own module so the component file exports only a
// component (React Fast Refresh friendliness).
import { createContext } from "react";
import type { Resolver } from "./resolve";

export interface LinkContextValue {
  resolver: Resolver;
  /** Vault-relative, forward-slash path of the note being previewed. */
  fromRel: string;
  /** Navigate to a note by absolute path. */
  onNavigate: (path: string) => void;
  /** Create (or open) a note for an unresolved link target. */
  onCreate: (target: string) => void;
}

export const LinkContext = createContext<LinkContextValue | null>(null);
