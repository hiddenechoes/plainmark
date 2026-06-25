import { createContext, useContext, useEffect, useMemo, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import remarkFrontmatter from "remark-frontmatter";
import rehypeKatex from "rehype-katex";
// Bundled KaTeX stylesheet + fonts — never loaded from a CDN (SPEC §8.7).
import "katex/dist/katex.min.css";
import { remarkWikiEmbed } from "../lib/markdown/remarkWikiEmbed";
import { remarkWikiLink } from "../lib/markdown/remarkWikiLink";
import { resolveImagePath, type PreviewLocation } from "../lib/markdown/resolveImage";
import { createResolver } from "../lib/links/resolve";
import { dirname, relativeTo } from "../lib/path";
import { readImage, runQuery, type NoteMeta } from "../lib/tauri";
import { LinkContext, type LinkContextValue } from "../lib/links/context";
import { QueryContext, type QueryContextValue, type ToggleArgs } from "../lib/query/context";
import { today } from "../lib/dailyNote";
import { WikiLink } from "./WikiLink";
import { Mermaid } from "./Mermaid";
import { QueryBlock } from "./QueryBlock";

// Where the previewed note lives, so image references resolve correctly. Held
// in context to avoid threading it through react-markdown's component map.
const LocationContext = createContext<PreviewLocation | null>(null);

// Session cache of resolved image data URLs, keyed by absolute path. Attachments
// get collision-safe unique names, so cached entries never go stale in a session.
const imageCache = new Map<string, string>();

function PreviewImage({ src, alt, isWiki }: { src: string; alt: string; isWiki: boolean }) {
  const loc = useContext(LocationContext);
  // Data URLs are self-contained, and remote URLs are never fetched (offline
  // guarantee), so both outcomes derive from the props during render. Only a
  // resolvable vault path needs an async load from the backend.
  const isData = src.startsWith("data:");
  const abs = useMemo(
    () => (isData || !loc ? null : resolveImagePath(loc, src, isWiki)),
    [isData, loc, src, isWiki],
  );
  // The cache is a plain module-level Map, so a hit is read during render — no
  // effect or extra state needed.
  const cachedUrl = abs ? (imageCache.get(abs) ?? null) : null;
  // Async load outcome, tagged with the `abs` it belongs to so a result from a
  // previous image is ignored once `src` changes (a null url means it failed).
  const [loaded, setLoaded] = useState<{ abs: string; url: string | null } | null>(null);

  useEffect(() => {
    if (!abs || imageCache.has(abs)) return;
    let active = true;
    readImage(abs)
      .then((url) => {
        if (!active) return;
        imageCache.set(abs, url);
        setLoaded({ abs, url });
      })
      .catch(() => {
        if (active) setLoaded({ abs, url: null });
      });
    return () => {
      active = false;
    };
  }, [abs]);

  const dataUrl = isData ? src : (cachedUrl ?? (loaded?.abs === abs ? loaded.url : null));
  if (!dataUrl) {
    // Covers not-yet-loaded, load failures, and unresolvable refs such as
    // remote URLs (never fetched — offline guarantee).
    return <span className="img-missing">{alt || src}</span>;
  }
  return <img className="preview-img" src={dataUrl} alt={alt} />;
}

const components: Components = {
  // Wiki links are emitted by remarkWikiLink as `<a data-wikilink>`; render them
  // via WikiLink (which resolves against the index). Real links fall through.
  a({ node, href, children }) {
    const props = node?.properties;
    if (props && props["data-wikilink"]) {
      const target = typeof props["data-target"] === "string" ? props["data-target"] : "";
      const heading = typeof props["data-heading"] === "string" ? props["data-heading"] : "";
      return (
        <WikiLink target={target} heading={heading === "" ? null : heading}>
          {children}
        </WikiLink>
      );
    }
    return <a href={typeof href === "string" ? href : undefined}>{children}</a>;
  },
  img({ node, src, alt }) {
    const isWiki = node?.properties?.["data-embed"] === "wiki";
    return (
      <PreviewImage
        src={typeof src === "string" ? src : ""}
        alt={typeof alt === "string" ? alt : ""}
        isWiki={isWiki}
      />
    );
  },
  // Intercept fenced code at the <pre> wrapper so a mermaid block renders as a
  // diagram (no <pre>), while ordinary code blocks keep their <pre><code>.
  pre({ node, children }) {
    const code = node?.children?.[0];
    if (code && code.type === "element" && code.tagName === "code") {
      const classes = code.properties?.className;
      const first = code.children[0];
      const value = first && first.type === "text" ? first.value : "";
      if (Array.isArray(classes) && classes.includes("language-mermaid")) {
        return <Mermaid code={value} />;
      }
      // A ` ```query ` block renders a live task list (SPEC §8.5).
      if (Array.isArray(classes) && classes.includes("language-query")) {
        return <QueryBlock source={value} />;
      }
    }
    return <pre>{children}</pre>;
  },
};

interface PreviewProps {
  content: string;
  vaultRoot: string;
  notePath: string;
  /** Link-target snapshot for resolving `[[wikilinks]]`. */
  targets?: NoteMeta[];
  /** Navigate to a note (by absolute path) when a resolved link or a query
   * result is clicked. `line` (1-based) is supplied by query-result links. */
  onNavigate?: (path: string, line?: number) => void;
  /** Create (or open) a note when an unresolved link is clicked. */
  onCreate?: (target: string) => void;
  /** Toggle a query result's checkbox in its source file. Owned by the app so it
   * can guard the currently-open note's unsaved edits before writing (§8.5). */
  onToggleTask?: (args: ToggleArgs) => Promise<boolean>;
}

const noop = () => {};
const defaultToggle = (): Promise<boolean> =>
  Promise.reject(new Error("task toggling is unavailable"));

/** Split-pane rendered Markdown view (SPEC §8.1 split preview). Raw HTML is not
 * passed through (no `rehype-raw`), so injected markup is escaped, not executed. */
export function Preview({
  content,
  vaultRoot,
  notePath,
  targets = [],
  onNavigate = noop,
  onCreate = noop,
  onToggleTask = defaultToggle,
}: PreviewProps) {
  const loc = useMemo<PreviewLocation>(
    () => ({ vaultRoot, noteDir: dirname(notePath) }),
    [vaultRoot, notePath],
  );
  const linkCtx = useMemo<LinkContextValue>(() => {
    const fromRel = relativeTo(vaultRoot, notePath).replace(/\\/g, "/");
    return { resolver: createResolver(targets), fromRel, onNavigate, onCreate };
  }, [vaultRoot, notePath, targets, onNavigate, onCreate]);
  const queryCtx = useMemo<QueryContextValue>(
    () => ({
      // `today` is read at run time so a long-open window still uses the right
      // local date; the backend never reads a clock (mirrors daily notes, §8.3).
      run: (src) => runQuery(src, today()),
      onNavigate: (path, line) => onNavigate(path, line),
      onToggle: (args) => onToggleTask(args),
    }),
    [onNavigate, onToggleTask],
  );

  return (
    <div className="preview-pane">
      <div className="preview-content">
        <QueryContext.Provider value={queryCtx}>
          <LinkContext.Provider value={linkCtx}>
            <LocationContext.Provider value={loc}>
              <Markdown
                remarkPlugins={[
                  remarkFrontmatter,
                  remarkGfm,
                  // Obsidian-style: a single newline is a line break, not a space
                  // (CommonMark would otherwise reflow consecutive lines together).
                  remarkBreaks,
                  remarkMath,
                  remarkWikiEmbed,
                  remarkWikiLink,
                ]}
                rehypePlugins={[rehypeKatex]}
                components={components}
              >
                {content}
              </Markdown>
            </LocationContext.Provider>
          </LinkContext.Provider>
        </QueryContext.Provider>
      </div>
    </div>
  );
}
