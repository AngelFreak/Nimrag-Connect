import type { SendInfo } from '../../api/generated';
import { fmtAgo } from '../watch/format';

/** One short phrase for a list row: where the workout is on its way to the watch. */
export function sendLabel(send: SendInfo, now: number = Date.now()): string {
  switch (send.state) {
    case 'idle':
      return 'Not sent';
    case 'queued':
      return 'Waiting for the phone';
    case 'sent': {
      const ago = fmtAgo(send.finished_at, now);
      return ago ? `On the watch, ${ago}` : 'On the watch';
    }
    case 'failed':
      return 'Could not be sent';
  }
}

/** The sentence under "Watch" in the builder: what happened and what happens next. */
export function sendSentence(send: SendInfo, now: number = Date.now()): string {
  switch (send.state) {
    case 'idle':
      return 'Not sent to the watch yet.';
    case 'queued':
      return 'Waiting for the phone. The Nimrag app sends it the next time it is connected to the watch.';
    case 'sent': {
      const ago = fmtAgo(send.finished_at, now);
      return ago ? `Sent to the watch ${ago}. Open the workouts on the watch to start it.` : 'Sent to the watch. Open the workouts on the watch to start it.';
    }
    case 'failed':
      return send.error ? `The watch did not take it: ${send.error}` : 'The workout could not be sent to the watch.';
  }
}
