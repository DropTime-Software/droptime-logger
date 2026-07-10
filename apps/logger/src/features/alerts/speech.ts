/**
 * Text-to-speech for spoken alerts, via the browser's SpeechSynthesis API
 * (available in the Tauri webview and browsers alike). Best-effort: silently
 * no-ops where unavailable. READ-ONLY — it only talks.
 */

export function canSpeak(): boolean {
  return typeof window !== 'undefined' && 'speechSynthesis' in window;
}

export function speak(phrase: string): void {
  const text = phrase.trim();
  if (!text || !canSpeak()) return;
  try {
    const utter = new SpeechSynthesisUtterance(text);
    utter.rate = 1;
    utter.pitch = 1;
    utter.volume = 1;
    // Drop any queued backlog so alerts stay timely rather than stacking up.
    window.speechSynthesis.cancel();
    window.speechSynthesis.speak(utter);
  } catch {
    /* ignore */
  }
}
