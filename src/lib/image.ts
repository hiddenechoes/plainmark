// Clipboard/drag-drop image helpers. These run in the webview, which never
// touches the filesystem — bytes are handed to the `save_attachment` Tauri
// command (see .claude/rules/frontend.md, SPEC §8.9).

/** Pick a file extension for a pasted/dropped image, preferring the MIME type
 * (clipboard images usually have no name) and falling back to the file name. */
export function imageExt(file: File): string {
  const sub = file.type.split("/")[1]?.toLowerCase();
  if (sub) {
    if (sub === "jpeg") return "jpg";
    if (sub === "svg+xml") return "svg";
    if (/^[a-z0-9]+$/.test(sub)) return sub;
  }
  const nameExt = file.name.split(".").pop()?.toLowerCase();
  if (nameExt && /^[a-z0-9]+$/.test(nameExt)) return nameExt;
  return "png";
}

/** Read a file as a base64 string, chunked to avoid blowing the call stack on
 * large images. */
export async function fileToBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Extract image files from a paste/drop `DataTransfer`, looking in both
 * `files` and `items` (clipboard images often only surface as items). */
export function imageFilesFrom(dt: DataTransfer | null): File[] {
  if (!dt) return [];
  const files = Array.from(dt.files).filter((f) => f.type.startsWith("image/"));
  if (files.length > 0) return files;
  const fromItems: File[] = [];
  for (const item of Array.from(dt.items ?? [])) {
    if (item.kind === "file" && item.type.startsWith("image/")) {
      const file = item.getAsFile();
      if (file) fromItems.push(file);
    }
  }
  return fromItems;
}
