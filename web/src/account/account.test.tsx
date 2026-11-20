import { describe, expect, it } from 'vitest';
import { ApiError, NetworkError } from '../api/client';
import { classifyStepError, describeAccountError, describeLoginError, describeStepError } from './errors';
import { cleanCode, normaliseUsername } from './passwords';
import { qrPath } from './QrCode';

describe('QR code', () => {
  it('is drawn from the text in the browser as one path inside a quiet zone', () => {
    const { size, path } = qrPath('otpauth://totp/Nimrag%20Connect:demo?secret=KRSXG5CTMVRXEZLU&issuer=Nimrag%20Connect');
    // Version 5 is 37 modules; any valid size is 17 + 4 * version, plus the 4-module quiet zone on each side.
    expect((size - 8 - 17) % 4).toBe(0);
    // The top-left finder pattern starts with a row of seven dark modules, after the quiet zone.
    expect(path.startsWith('M4 4h7v1h-7z')).toBe(true);
    expect(qrPath('otpauth://totp/x?secret=A').path).not.toBe(path);
    expect(qrPath('same').path).toBe(qrPath('same').path);
  });
});

describe('input rules', () => {
  it('keeps six digits of a pasted code and lower-cases the username', () => {
    expect(cleanCode('123 456 789')).toBe('123456');
    expect(cleanCode('12ab34')).toBe('1234');
    expect(normaliseUsername('  Boss.One ')).toBe('boss.one');
  });
});

describe('error wording', () => {
  it('one message for a wrong username, password or code; a wait message for rate limiting', () => {
    expect(describeLoginError(new ApiError(401, 'wrong username, password or code'))).toBe('Wrong username, password or code.');
    expect(describeLoginError(new ApiError(429, 'too many attempts'))).toMatch(/Wait a little/);
    expect(describeLoginError(new NetworkError())).toMatch(/Could not reach the server/);
  });
  it('tells a wrong code from an expired step, whatever the status', () => {
    expect(classifyStepError(new ApiError(401, 'wrong code'))).toBe('wrong-code');
    expect(classifyStepError(new ApiError(422, 'wrong code'))).toBe('wrong-code');
    expect(classifyStepError(new ApiError(401, 'too many wrong codes; log in again'))).toBe('expired');
    expect(classifyStepError(new ApiError(422, 'too many wrong codes; start again'))).toBe('expired');
    expect(classifyStepError(new ApiError(401, 'this step has expired; log in again'))).toBe('expired');
    expect(classifyStepError(new ApiError(400, 'password must have at least 10 characters'))).toBe('rejected');
    expect(describeStepError(new ApiError(400, 'password must have at least 10 characters'))).toBe('password must have at least 10 characters');
    expect(classifyStepError(new ApiError(429, 'x'))).toBe('rate-limited');
  });
  it('a refused password in a signed-in form is 403', () => {
    expect(describeAccountError(new ApiError(403, 'wrong password'))).toBe('Wrong password.');
    expect(describeAccountError(new ApiError(429, 'x'))).toMatch(/Too many attempts/);
  });
});
