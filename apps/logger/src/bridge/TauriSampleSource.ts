/**
 * TauriSampleSource — a SampleSource (types.ts) backed by the Rust capture engine.
 *
 * It wraps `start_session` / `resume_session`, wiring a `tauri::ipc::Channel` to the
 * console's `SourceListener`. The console cannot tell this apart from the in-process
 * SimulatorSource — that is the whole point of the shared interface.
 *
 * Note: Tauri has no generic "stop capture" command — the session is torn down via
 * `finish_session` / `abandon_session` (owned by the store). `stop()` here just
 * detaches the channel so no further events reach the listener.
 */
import { Channel } from '@tauri-apps/api/core';
import type { SampleSource, SourceInfo, SourceListener } from '@droptime/roast-console';
import { ipc } from './ipc';
import type { MarkerEvent, SampleEvent, StartSessionArgs } from './dto';

export class TauriSampleSource implements SampleSource {
  readonly info: SourceInfo;

  /** UUIDv7 minted by start_session (or the resumed roast's uuid). */
  roastUuid?: string;
  /** seq the replay driver reattached from, when resuming. */
  resumedFromSeq?: number;
  /**
   * CONTRACTS §6 `marker` stream events (backend-initiated marker changes,
   * v0.1.0: auto CHARGE/DROP). Outside `SourceListener` because the shared
   * contract interface doesn't know about markers — the SessionProvider wires
   * this before `start()`.
   */
  onMarker?: (ev: MarkerEvent) => void;

  private readonly startArgs: StartSessionArgs;
  private readonly resumeUuid?: string;
  private stopped = false;

  /**
   * @param info      display metadata for the source
   * @param startArgs args for a fresh `start_session`
   * @param resumeUuid when set, `start()` reattaches to this roast via `resume_session`
   */
  constructor(info: SourceInfo, startArgs: StartSessionArgs, resumeUuid?: string) {
    this.info = info;
    this.startArgs = startArgs;
    this.resumeUuid = resumeUuid;
  }

  async start(listener: SourceListener): Promise<void> {
    const channel = new Channel<SampleEvent>();
    channel.onmessage = (ev) => {
      if (this.stopped) return;
      if (ev.type === 'sample') {
        listener.onSample({
          seq: ev.seq,
          sessionSec: ev.sessionSec,
          btF: ev.btF,
          etF: ev.etF,
          ambientF: ev.ambientF,
          heater: ev.heater,
          fan: ev.fan,
          drum: ev.drum,
        });
      } else if (ev.type === 'marker') {
        this.onMarker?.(ev);
      } else {
        listener.onStatus({
          kind: ev.kind,
          message: ev.message,
          atSessionSec: ev.atSessionSec,
        });
      }
    };

    if (this.resumeUuid) {
      const res = await ipc.resumeSession({ roastUuid: this.resumeUuid }, channel);
      this.roastUuid = this.resumeUuid;
      this.resumedFromSeq = res.resumedFromSeq;
    } else {
      const res = await ipc.startSession(this.startArgs, channel);
      this.roastUuid = res.roastUuid;
    }
  }

  async stop(): Promise<void> {
    this.stopped = true;
  }
}
