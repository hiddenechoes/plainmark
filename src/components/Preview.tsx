import { createContext, useContext, useEffect, useMemo, useState } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkFrontmatter from "remark-frontmatter";
import rehypeKatex from "rehype-katex";
// Bundled KaTeX stylesheet + fonts — never loaded from a CDN (SPEC §8.7).
import "katex/dist/katex.min.css";
import { remarkWikiEmbed } from "../lib/markdown/remarkWikiEmbed";
import { resolveImagePath, type PreviewLocation } from "../lib/markdown/resolveImage";
import { dirname } from "../lib/path";
import { readImage } from "../lib/tauri";
import { Mermaid } from "./Mermaid";

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
      const isMermaid = Array.isArray(classes) && classes.includes("language-mermaid");
      if (isMermaid) {
        const first = code.children[0];
        const value = first && first.type === "text" ? first.value : "";
        return <Mermaid code={value} />;
      }
    }
    return <pre>{children}</pre>;
  },
};

interface PreviewProps {
  content: string;
  vaultRoot: string;
  notePath: string;
}

/** Split-pane rendered Markdown view (SPEC §8.1 split preview). Raw HTML is not
 * passed through (no `rehype-raw`), so injected markup is escaped, not executed. */
export function Preview({ content, vaultRoot, notePath }: PreviewProps) {
  const loc = useMemo<PreviewLocation>(
    () => ({ vaultRoot, noteDir: dirname(notePath) }),
    [vaultRoot, notePath],
  );
  return (
    <div className="preview-pane">
      <div className="preview-content">
        <LocationContext.Provider value={loc}>
          <Markdown
            remarkPlugins={[remarkFrontmatter, remarkGfm, remarkMath, remarkWikiEmbed]}
            rehypePlugins={[rehypeKatex]}
            components={components}
          >
            {content}
          </Markdown>
        </LocationContext.Provider>
      </div>
    </div>
  );
}
