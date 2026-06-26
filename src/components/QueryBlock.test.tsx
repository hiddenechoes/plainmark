// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryBlock } from "./QueryBlock";
import { QueryContext, type QueryContextValue } from "../lib/query/context";
import type { QueryResponse, TaskResult } from "../lib/tauri";

// Capture the `index://updated` subscriber so a test can fire a live update, and
// stub the error markers — QueryBlock pulls these from the Tauri bridge module,
// which we don't want to load (it imports the Tauri runtime).
let indexListener: (() => void) | null = null;
vi.mock("../lib/tauri", () => ({
  CHANGED_ON_DISK: "changed-on-disk",
  TASK_MISMATCH: "task-mismatch",
  onIndexUpdated: (cb: () => void) => {
    indexListener = cb;
    return Promise.resolve(() => {
      indexListener = null;
    });
  },
}));

function task(over: Partial<TaskResult> = {}): TaskResult {
  return {
    path: "/vault/Work/Plan.md",
    relPath: "Work/Plan.md",
    title: "Plan",
    line: 7,
    text: "Write the spec",
    done: false,
    due: null,
    tags: [],
    ...over,
  };
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  indexListener = null;
  vi.restoreAllMocks();
});

async function mount(ctx: QueryContextValue, source = "not done") {
  await act(async () => {
    root.render(
      <QueryContext.Provider value={ctx}>
        <QueryBlock source={source} />
      </QueryContext.Provider>,
    );
  });
}

function ctxWith(
  run: (source: string) => Promise<QueryResponse>,
  onToggle = vi.fn().mockResolvedValue(true),
  onNavigate = vi.fn(),
): QueryContextValue {
  return { run, onNavigate, onToggle };
}

describe("QueryBlock", () => {
  it("runs the source and renders each matching task with a source link", async () => {
    const run = vi.fn().mockResolvedValue({
      error: null,
      tasks: [task(), task({ line: 12, text: "Ship it", done: true })],
    });
    await mount(ctxWith(run));

    expect(run).toHaveBeenCalledWith("not done");
    const items = host.querySelectorAll(".query-result");
    expect(items).toHaveLength(2);
    expect(host.textContent).toContain("Write the spec");
    // Each result links to its source file + line.
    expect(host.textContent).toContain("Work/Plan.md:7");
    expect(host.textContent).toContain("Work/Plan.md:12");
    // A done task reflects its checked state.
    const checks = host.querySelectorAll<HTMLInputElement>(".query-check");
    expect(checks[0].checked).toBe(false);
    expect(checks[1].checked).toBe(true);
  });

  it("clicking a result's source link navigates to its file and line", async () => {
    const onNavigate = vi.fn();
    const run = vi.fn().mockResolvedValue({ error: null, tasks: [task()] });
    await mount(ctxWith(run, vi.fn().mockResolvedValue(true), onNavigate));

    const link = host.querySelector<HTMLButtonElement>(".query-source")!;
    act(() => link.click());
    expect(onNavigate).toHaveBeenCalledWith("/vault/Work/Plan.md", 7);
  });

  it("toggling a checkbox writes back, then the live update flips it", async () => {
    const onToggle = vi.fn().mockResolvedValue(true);
    // First run: open. After the write-back + index update, the task is done.
    const run = vi
      .fn()
      .mockResolvedValueOnce({ error: null, tasks: [task()] })
      .mockResolvedValueOnce({ error: null, tasks: [task({ done: true })] });
    await mount(ctxWith(run, onToggle));

    const check = host.querySelector<HTMLInputElement>(".query-check")!;
    expect(check.checked).toBe(false);

    await act(async () => {
      check.click();
    });
    // Re-verified write-back: the expected text + current status are sent.
    expect(onToggle).toHaveBeenCalledWith({
      path: "/vault/Work/Plan.md",
      line: 7,
      text: "Write the spec",
      done: false,
    });

    // The backend emits `index://updated`; firing it re-runs and flips the box.
    await act(async () => {
      indexListener?.();
    });
    expect(run).toHaveBeenCalledTimes(2);
    expect(host.querySelector<HTMLInputElement>(".query-check")!.checked).toBe(true);
  });

  it("renders a malformed query as an inline error, never crashing", async () => {
    const run = vi.fn().mockResolvedValue({ error: "unknown directive: nope", tasks: [] });
    await mount(ctxWith(run), "nope");

    expect(host.querySelector(".query-error")).not.toBeNull();
    expect(host.textContent).toContain("unknown directive: nope");
    expect(host.querySelector(".query-result")).toBeNull();
  });

  it("re-runs when the index updates so results stay live", async () => {
    const run = vi
      .fn()
      .mockResolvedValueOnce({ error: null, tasks: [task()] })
      .mockResolvedValueOnce({ error: null, tasks: [task(), task({ line: 9, text: "New one" })] });
    await mount(ctxWith(run));
    expect(host.querySelectorAll(".query-result")).toHaveLength(1);

    await act(async () => {
      indexListener?.();
    });
    expect(host.querySelectorAll(".query-result")).toHaveLength(2);
    expect(host.textContent).toContain("New one");
  });

  it("surfaces a write-back mismatch as a friendly message", async () => {
    const onToggle = vi.fn().mockRejectedValue(new Error("task-mismatch: line 7 changed"));
    const run = vi.fn().mockResolvedValue({ error: null, tasks: [task()] });
    await mount(ctxWith(run, onToggle));

    await act(async () => {
      host.querySelector<HTMLInputElement>(".query-check")!.click();
    });
    expect(host.querySelector(".query-toggle-error")?.textContent).toContain("changed on disk");
  });
});
