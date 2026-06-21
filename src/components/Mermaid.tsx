import { useEffect, useRef, useState } from "react";
import mermaid from "mermaid";

// Mermaid is a bundled npm dependency — it never fetches anything at runtime
// (SPEC §8.4: diagrams render with the network disabled). `securityLevel:
// "strict"` keeps the generated SVG sanitized, in line with the renderer's
// no-arbitrary-HTML guarantee.
let initialized = false;
let counter = 0;

function ensureInitialized(): void {
  if (initialized) return;
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: "dark",
    fontFamily: "inherit",
  });
  initialized = true;
}

/** Render a single ` ```mermaid ` block to inline SVG. */
export function Mermaid({ code }: { code: string }) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    ensureInitialized();
    const id = `plainmark-mermaid-${(counter += 1)}`;
    mermaid
      .render(id, code)
      .then(({ svg }) => {
        if (active && hostRef.current) {
          hostRef.current.innerHTML = svg;
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (active) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      active = false;
    };
  }, [code]);

  if (error) {
    return <pre className="mermaid-error">{error}</pre>;
  }
  return <div className="mermaid" ref={hostRef} />;
}
