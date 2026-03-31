/** Open an external URL — uses Tauri opener plugin when available, falls back to window.open. */
export async function openUrl(url: string): Promise<void> {
  try {
    const { openUrl: tauriOpen } = await import("@tauri-apps/plugin-opener");
    await tauriOpen(url);
  } catch {
    window.open(url, "_blank");
  }
}
