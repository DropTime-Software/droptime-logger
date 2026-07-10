/**
 * features/reference — live cue layer for LiveScreen (CONTRACTS §8, owner:
 * reference). Playback-aid only: cues inform the human — nothing here actuates
 * the machine or auto-marks the live roast.
 *
 * The ghost curve itself is already wired (LiveScreen passes
 * `state.reference?.curve` to LiveRoastChart's `ghost` prop). This layer adds:
 *   1. a small reference chip (label + ghost affordance),
 *   2. an upcoming-event cue chip driven by pure `upcomingCues` math evaluated
 *      against the live derived clock (elapsedT / charged),
 *   3. a spoken cue + WebAudio tick (on by default; `sound.cues = 'off'`
 *      disables) routed through the shared alerts sound gate, fired once per
 *      reference event, only while recording (never during preheat).
 */
import { useEffect, useRef } from 'react';
import {
  formatCue,
  tokens,
  upcomingCues,
  type ReferenceCue,
} from '@droptime/roast-console';
import { useSession } from '../../state/SessionProvider';
import { playCue, soundCuesEnabled } from '../alerts/sound';
import { speak } from '../alerts/speech';

/** Announce a milestone this many roast-seconds ahead of the reference time. */
const SPEAK_LEAD_SEC = 15;

export function ReferenceLayer() {
  const { state, derived, capturing } = useSession();
  const reference = state.reference;
  const { elapsedT, charged } = derived;

  const announced = useRef<Set<string>>(new Set());

  // Reset the fired-once set when the session or the chosen reference changes.
  useEffect(() => {
    announced.current = new Set();
  }, [state.roastUuid, reference?.roastUuid]);

  const cues: ReferenceCue[] =
    reference && charged ? upcomingCues(reference.markers, reference.curve, elapsedT) : [];
  const next = cues[0] ?? null;
  const armed = !!next && (next.terminal || next.secondsUntil <= SPEAK_LEAD_SEC);

  // Fire the one-shot spoken/tick cue as a milestone comes due — only while
  // recording (never during preheat), once per reference event.
  useEffect(() => {
    if (!reference || !capturing || !charged) return;
    if (!next || !armed) return;
    if (announced.current.has(next.kind)) return;
    announced.current.add(next.kind);
    // Route through the shared alerts sound gate (unset = ON; only 'off'
    // disables), kept current by useAlerts + SettingsScreen. playCue self-gates.
    if (soundCuesEnabled()) speak(formatCue(next));
    playCue('tick');
  }, [reference, capturing, charged, next?.kind, armed]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!reference) return null;

  return (
    <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
      {/* reference chip */}
      <div className="flex min-w-0 items-center gap-2 text-ink/55">
        <span
          aria-hidden
          className="inline-block h-[3px] w-5 shrink-0 rounded"
          style={{ backgroundColor: tokens.ghost }}
        />
        <span className="truncate">
          <span className="font-semibold text-pine">Reference</span>{' '}
          <span className="text-ink/45">{reference.label}</span>
        </span>
        <span className="shrink-0 text-[10px] font-semibold uppercase tracking-wide text-ink/35">
          ghost
        </span>
      </div>

      {/* upcoming-event cue chip */}
      {next ? (
        <div
          role="status"
          aria-live="polite"
          className="flex shrink-0 items-center gap-2 rounded-full px-3 py-1 font-semibold"
          style={{
            backgroundColor: armed ? tokens.lemon : tokens.surface,
            color: tokens.pine,
            boxShadow: `inset 0 0 0 1px ${tokens.hairline}`,
          }}
        >
          <span
            aria-hidden
            className="inline-block h-1.5 w-1.5 rounded-full"
            style={{ backgroundColor: next.terminal ? tokens.warn : tokens.ghost }}
          />
          <span>{formatCue(next)}</span>
        </div>
      ) : charged ? (
        <span className="shrink-0 text-ink/35">No upcoming reference cues</span>
      ) : (
        <span className="shrink-0 text-ink/35">Ghost aligned at charge · cues begin after charge</span>
      )}
    </div>
  );
}
