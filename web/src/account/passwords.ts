/** Same rules as the server (`MIN_PASSWORD_LEN`, `normalise_username`). */
export const MIN_PASSWORD_LEN = 10;
export const USERNAME_MIN = 3;
export const USERNAME_MAX = 32;

/** Why the new password cannot be sent yet, or null when it can. */
export function passwordProblem(password: string, confirm: string): string | null {
  if ([...password].length < MIN_PASSWORD_LEN) return `Use at least ${MIN_PASSWORD_LEN} characters.`;
  if (password !== confirm) return 'The two passwords are not the same.';
  return null;
}

/** The server lower-cases and trims; so does the form, so what is checked is what is sent. */
export function normaliseUsername(raw: string): string {
  return raw.trim().toLowerCase();
}

/** Why the username is not valid, or null. */
export function usernameProblem(raw: string): string | null {
  const name = normaliseUsername(raw);
  const length = [...name].length;
  if (length < USERNAME_MIN || length > USERNAME_MAX) return `Use ${USERNAME_MIN} to ${USERNAME_MAX} characters.`;
  if (!/^[a-z0-9._-]+$/.test(name)) return "Use only the letters a to z, digits, '.', '_' and '-'.";
  return null;
}

/** Keeps the digits a person typed or pasted ("123 456" becomes "123456"), at most six. */
export function cleanCode(raw: string): string {
  return raw.replace(/\D/g, '').slice(0, 6);
}
