// Helpers for the image attachment flow. Image bytes are read by Rust (system
// clipboard / dropped file paths), so the webview only needs to recognise which
// dropped paths are images (SPEC §8.9).

const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"]);

/** True if `path` ends in a known image extension. */
export function isImagePath(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase();
  return ext !== undefined && IMAGE_EXTS.has(ext);
}
