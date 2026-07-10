/**
 * Synthesized alert cues — no audio assets, generated with WebAudio so they
 * ship in the strict-CSP bundle. `tick` is a single soft blip; `chime` is a
 * gentle rising two-tone. A module-level gate honors the global `sound.cues`
 * setting, so every cue (alerts and any projected-FC cue that routes through
 * `playCue`) falls silent together when the user turns sounds off.
 */
import type { AlertSound } from './types';

let soundEnabled = true;
let ctx: AudioContext | null = null;

/** Wire the global `sound.cues` setting to every synthesized cue. */
export function setSoundCuesEnabled(on: boolean): void {
  soundEnabled = on;
}

export function soundCuesEnabled(): boolean {
  return soundEnabled;
}

type AudioCtor = typeof AudioContext;

function audioContext(): AudioContext | null {
  if (typeof window === 'undefined') return null;
  const Ctor: AudioCtor | undefined =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: AudioCtor }).webkitAudioContext;
  if (!Ctor) return null;
  try {
    if (!ctx) ctx = new Ctor();
    // Autoplay policies suspend the context until a gesture; roasting is
    // interactive so this generally resumes immediately.
    if (ctx.state === 'suspended') void ctx.resume();
    return ctx;
  } catch {
    return null;
  }
}

function blip(ac: AudioContext, start: number, freq: number, dur: number, peak: number): void {
  const osc = ac.createOscillator();
  const gain = ac.createGain();
  osc.type = 'sine';
  osc.frequency.setValueAtTime(freq, start);
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(peak, start + 0.012);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + dur);
  osc.connect(gain).connect(ac.destination);
  osc.start(start);
  osc.stop(start + dur + 0.02);
}

/** Play a cue, respecting the global sound gate. Safe to call anywhere. */
export function playCue(kind: AlertSound): void {
  if (!soundEnabled) return;
  const ac = audioContext();
  if (!ac) return;
  const now = ac.currentTime;
  if (kind === 'tick') {
    blip(ac, now, 880, 0.09, 0.18);
  } else {
    blip(ac, now, 660, 0.16, 0.2);
    blip(ac, now + 0.16, 988, 0.28, 0.2);
  }
}
