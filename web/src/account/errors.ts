import { ApiError, NetworkError } from '../api/client';
import { errorMessage } from '../pages/system/errors';

const NETWORK = 'Could not reach the server. Check your connection and try again.';
const WAIT = 'Too many attempts. Wait a little and try again.';

/** The sign-in form: a wrong username, password or code is one and the same answer. */
export function describeLoginError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 401) return 'Wrong username, password or code.';
    if (error.status === 429) return WAIT;
    return `The server answered with an error (${error.status}). Try again.`;
  }
  if (error instanceof NetworkError) return NETWORK;
  return 'Something went wrong. Try again.';
}

/** What went wrong with a code or a password at the second step of a flow that holds a pending token. */
export type StepFailure = 'wrong-code' | 'expired' | 'rate-limited' | 'rejected' | 'network' | 'other';

/**
 * The server answers a wrong code and an expired step with the same status (401 on login and
 * setup, 422 on re-enrolment), so the message tells them apart: "wrong code" can be tried again,
 * anything else means the step is gone.
 */
export function classifyStepError(error: unknown): StepFailure {
  if (error instanceof NetworkError) return 'network';
  if (!(error instanceof ApiError)) return 'other';
  if (error.status === 429) return 'rate-limited';
  if ((error.status === 401 || error.status === 422) && error.message === 'wrong code') return 'wrong-code';
  if ([401, 404, 410, 422].includes(error.status)) return 'expired';
  if (error.status === 400 || error.status === 409) return 'rejected';
  return 'other';
}

/** Plain words for a failed step; `rejected` carries the server's own message (a password rule). */
export function describeStepError(error: unknown): string {
  switch (classifyStepError(error)) {
    case 'wrong-code':
      return 'That code is not right. Check the code in your authenticator app and try again.';
    case 'expired':
      return 'This step has expired. Start again.';
    case 'rate-limited':
      return WAIT;
    case 'network':
      return NETWORK;
    case 'rejected':
      return errorMessage(error);
    default:
      return 'Something went wrong. Try again.';
  }
}

/** A refused request from a signed-in form (password or two-factor change): a wrong password is 403. */
export function describeAccountError(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 403) return 'Wrong password.';
    if (error.status === 429) return WAIT;
  }
  return errorMessage(error);
}
