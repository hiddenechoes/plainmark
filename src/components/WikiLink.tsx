// Renders a single `[[wikilink]]` in the preview (SPEC §8.2, §8.8). Resolves
// against the link-target snapshot via the resolver in context: a resolved link
// is a clickable anchor that navigates; an unresolved one is muted and offers to
// create the note on click. A resolved link whose `#heading` doesn't exist is
// styled as a soft warning but still navigates to the note.
import { useContext, type ReactNode } from "react";
import { LinkContext } from "../lib/links/context";

interface WikiLinkProps {
  target: string;
  heading: string | null;
  children: ReactNode;
}

export function WikiLink({ target, heading, children }: WikiLinkProps) {
  const ctx = useContext(LinkContext);
  // Without a context (e.g. isolated render) just show the display text.
  if (!ctx) return <span className="wikilink">{children}</span>;

  const { resolver, fromRel, onNavigate, onCreate } = ctx;
  const { meta, headingOk } = resolver.status(target, heading, fromRel);

  if (meta) {
    const className = headingOk ? "wikilink" : "wikilink wikilink-bad-heading";
    const title = heading ? `${meta.relPath} › ${heading}` : meta.relPath;
    return (
      <a
        className={className}
        href="#"
        title={title}
        onClick={(e) => {
          e.preventDefault();
          onNavigate(meta.path);
        }}
      >
        {children}
      </a>
    );
  }

  return (
    <a
      className="wikilink wikilink-unresolved"
      href="#"
      role="button"
      title={`Create note “${target}”`}
      onClick={(e) => {
        e.preventDefault();
        onCreate(target);
      }}
    >
      {children}
    </a>
  );
}
