import { ApiError, NetworkError } from '../../api/client';

/** Plain-words text for a failed request; the server's own message when it sent one. */
export function errorMessage(error: unknown): string {
  if (error instanceof NetworkError) return 'Could not reach the server. Check the connection and try again.';
  if (error instanceof ApiError) return error.message;
  return 'Something went wrong. Try again.';
}
