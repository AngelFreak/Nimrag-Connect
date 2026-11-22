# Garmin Connect login + FIT upload: community-client spec (as of 2026-11-19)

Method: read-only static reading of public source. No Garmin endpoint was contacted, no credentials used, nothing was run or installed.
Claim tags: **[src file:line]** = from source, **[issue URL]** = from a GitHub issue/PR, **[unverified]** = not confirmed by anything I read.
All `file:line` refer to the clone at `.../scratchpad/garmin-research/pgc/` (python-garminconnect tag 0.3.20), paths relative to `garminconnect/` unless stated.

## 0. Provenance

| Item | Value |
|---|---|
| Repo | https://github.com/cyberjunky/python-garminconnect |
| Latest release | **0.3.20**, released 2026-10-23 (PyPI upload 2026-10-23T11:32Z) |
| Commit of release | `3eee7a4db6c2242967aae1780b8839599ea7dcbb` ("chore: release 0.3.20"). master HEAD on 2026-11-19 is `82bc1107` (2026-10-31, README-only commits after the release) |
| Licence | MIT, (c) 2020-2026 Ron Klinkien [src LICENSE:1-3]; PyPI metadata says MIT |
| Python deps | `curl_cffi>=0.15.0`, `requests>=2.33.0`, `ua-generator>=1.0`, python >=3.12 (PyPI metadata; pyproject.toml:9,11) |
| garth-ng | https://github.com/cyberfossa/garth-ng, PyPI `garth-ng` 1.1.0 (2026-04-12), repo has `2.0.0-alpha.1` as last release commit `5e450e3a` (2026-06-14), last commit `d81cc698` 2026-06-26 (dependabot only). Clone at `.../garmin-research/garth-ng`. MIT per PyPI |

Key architectural fact: **there is no OAuth1 and no consumer key/secret anymore.** The old garth flow (OAuth1 consumer key fetched from `thegarth.s3.amazonaws.com`, then OAuth1 -> OAuth2 exchange) is gone. 0.3.x authenticates with a **Garmin "DI" OAuth2 bearer token** obtained by exchanging a **CAS service ticket**. garth-ng uses the identical DI exchange (`garth-ng/src/garth/oauth.py:12-22`).

## 1. Login flow (default chain, strategy 1/2 = "mobile iOS")

`login()` tries 5 strategies in order: mobile+cffi, mobile+requests, widget+cffi, portal+cffi, portal+requests [src client/sso.py:154-163]. Only wrong-credentials errors and MFA stop the chain; 429/427/403/CAPTCHA/non-JSON fall through to the next [src client/sso.py:220-241]. After any strategy yields a token, it is verified with `GET /userprofile-service/socialProfile`; a 401/403 discards the token and moves on [src client/session.py:87-122, sso.py:207-218].

Hosts (domain `garmin.com`): `_sso = https://sso.garmin.com`, `_connectapi = https://connectapi.garmin.com`, `_ios_service_url = https://mobile.integration.garmin.com/gcm/ios`, `_di_token_url = https://diauth.garmin.com/di-oauth2-service/oauth/token` [src client/__init__.py (Client.__init__, "self._sso = ...", approx. lines 165-178); constants.py:17-18, 69].

### Step 1. Prime the session (HTML page load) [src client/sso.py:543-575]

```
GET https://sso.garmin.com/mobile/sso/en_US/sign-in?clientId=GCM_IOS_DARK&service=https%3A%2F%2Fmobile.integration.garmin.com%2Fgcm%2Fios
User-Agent: Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148
Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8
Accept-Language: en-US,en;q=0.9
```
- UA constant: [src constants.py:19-22]. `clientId` constant `GCM_IOS_DARK`: [src constants.py:17].
- Response body is not parsed. Only the **cookies** matter (`SESSION`, `__cf_bm`, `__cflb`, ...) [issue https://github.com/cyberjunky/python-garminconnect/pull/479]. A cookie jar must persist across steps 1-4 (and the MFA step).
- 429/427 -> rate limited; 403 -> "Cloudflare bot challenge" [src sso.py:566-571].
- Then sleep `uniform(2.0, 5.0)` s [src constants.py:50-51, sso.py:573-575].
- Why it matters: skipping this page made every credential POST return 429 "even when nothing is rate limited" [src CHANGELOG.md 0.3.20 "Fixed"; issue PR #479, tested live by the maintainer with the same TLS fingerprint: direct POST = 429, after page load = normal 401 INVALID_USERNAME_PASSWORD].

### Step 2. Credential POST [src client/sso.py:577-598, 451-462]

```
POST https://sso.garmin.com/mobile/api/login?clientId=GCM_IOS_DARK&locale=en-US&service=https%3A%2F%2Fmobile.integration.garmin.com%2Fgcm%2Fios
User-Agent: <same iOS UA>
Accept: application/json, text/plain, */*
Accept-Language: en-US,en;q=0.9
Content-Type: application/json
Origin: https://sso.garmin.com
Referer: https://sso.garmin.com/mobile/sso/en_US/sign-in?clientId=GCM_IOS_DARK&service=<urlencode(ios service url)>
{"username": "<email>", "password": "<pw>", "rememberMe": true, "captchaToken": ""}
```
timeout 30 s.

Response handling [src sso.py:464-527]:
- HTTP 429 or **427** (non-standard "blocked") -> rate-limit/blocked [sso.py:98-101, 464-468].
- HTTP 403 -> Cloudflare challenge, fall through [sso.py:470-474].
- Body must be JSON; `res["responseStatus"]["type"]` is the discriminator:
  - `SUCCESSFUL` -> `res["serviceTicketId"]` (a CAS ticket, `ST-...`) [sso.py:496-500]
  - `MFA_REQUIRED` -> MFA needed; method hint = `res["customerMfaInfo"]["mfaLastMethodUsed"]` (default `"email"`) [sso.py:485-494]
  - `INVALID_USERNAME_PASSWORD` -> bad credentials [sso.py:502-505]
  - `CAPTCHA_REQUIRED` -> treated as bot-challenge, fall through [sso.py:511-515]
  - also `res["error"]["status-code"] == "429"` buried in JSON body [sso.py:507-509]
- The JSON field names of the success response beyond `responseStatus.type`, `serviceTicketId`, `customerMfaInfo.mfaLastMethodUsed` are not used by the client. [src]

### Step 3 (only if MFA_REQUIRED). Submit code [src client/sso.py:905-1015]

How MFA is detected: `responseStatus.type == "MFA_REQUIRED"` in the Step 2 JSON (iOS/portal flows) [sso.py:485]. (The widget flow instead scrapes HTML, see section 1.4.)
Same cookie-jar session, same `params` and headers as step 2:

```
POST https://sso.garmin.com/mobile/api/mfa/verifyCode?clientId=GCM_IOS_DARK&locale=en-US&service=<ios service url>
(same headers as Step 2, incl. Content-Type: application/json and Referer)
{"mfaMethod": "<email|sms|... from customerMfaInfo.mfaLastMethodUsed>",
 "mfaVerificationCode": "<code>", "rememberMyBrowser": true, "reconsentList": [], "mfaSetup": false}
```
[src sso.py:921-927, 934-940, 963-968]. Success response has the same shape as step 2: `responseStatus.type == "SUCCESSFUL"` + `serviceTicketId` [sso.py:991-992]. Fallback: if the first endpoint fails/blocks, it retries on `/portal/api/mfa/verifyCode` with portal params (`clientId=GarminConnect`, `service=https://connect.garmin.com/app`) [sso.py:942-956]. Failed code leaves the pending session usable for another attempt (resume_login) [sso.py:268-272].
- Does Garmin send the code on the credential POST (iOS/portal flows)? The client takes no explicit action to trigger delivery in the JSON flows, so it presumably is sent as part of the step 2 response. **[unverified; no live trace]**. Widget flow had a case where no code was sent and the client explicitly calls `/sso/verifyMFA/mfaCode` [src sso.py:777-819; issue https://github.com/cyberjunky/python-garminconnect/issues/386].
- Two-step API: `login(return_on_mfa=True)` returns `("needs_mfa", client_state)`; client_state = {domain, flow, method, login_params, post_headers, service_url, csrf_token, impersonate, cookies[]} and is restorable in another process via `resume_login(state, code)` [src sso.py:284-315, 260-282; CHANGELOG 0.3.20 (#473)]. Implication for a Rust port: **the cookie jar + the TLS profile must survive between the two phases.**

### Step 4. CAS ticket -> DI OAuth2 token [src client/session.py:189-244, 173-187]

```
POST https://diauth.garmin.com/di-oauth2-service/oauth/token
Authorization: Basic base64("<client_id>:")        # secret is EMPTY, i.e. base64 of "GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2:"
Accept: application/json,text/html;q=0.9,*/*;q=0.8
Content-Type: application/x-www-form-urlencoded
Cache-Control: no-cache
User-Agent: GCM-Android-5.23
X-Garmin-User-Agent: com.garmin.android.apps.connectmobile/5.23; ; Google/sdk_gphone64_arm64/google; Android/33; Dalvik/2.1.0
X-Garmin-Paired-App-Version: 10861
X-Garmin-Client-Platform: Android
X-App-Ver: 10861
X-Lang: en
X-GCExperience: GC5
Accept-Language: en-US,en;q=0.9

client_id=<client_id>&service_ticket=<ST-...>&grant_type=https://connectapi.garmin.com/di-oauth2-service/oauth/grant/service_ticket&service_url=https://mobile.integration.garmin.com/gcm/ios
```
- Header set: [src client/transport.py:89-102 `_native_headers`]; Basic auth builder [src client/tokens.py:58-59]; request [src session.py:173-187, 205-215].
- Constants: `DI_GRANT_TYPE` [constants.py:70-72]; `DI_CLIENT_IDS` tried in this order until one returns 2xx with `access_token`: `GARMIN_CONNECT_MOBILE_ANDROID_DI_2025Q2`, `..._ANDROID_DI_2024Q4`, `GARMIN_CONNECT_MOBILE_ANDROID_DI`, `GARMIN_CONNECT_MOBILE_IOS_DI` [constants.py:73-78; session.py:205-235]. garth-ng tries the first three only (`garth-ng/src/garth/oauth.py:13-17`).
- `service_url` must equal the service URL used in the SSO login (ios URL for the mobile flow; `https://sso.garmin.com/sso/embed` for widget; `https://connect.garmin.com/app` for portal) [src session.py:198-199, sso.py:737, 774, 993-998].
- 429 -> `TooManyRequests`; any other non-2xx -> try next client id [session.py:216-226].
- Response JSON fields used: `access_token`, `refresh_token`. Client id is then read from the JWT's `client_id` claim (falls back to the id that worked) [session.py:228-231, tokens.py:88-93]. Other fields (`expires_in`, `scope`, `token_type`) are **not read**.
- The POST itself goes through curl_cffi (`impersonate="chrome"`) if installed, else plain `requests` [src session.py:162-171]. So plain TLS is accepted as a fallback; whether diauth actually needs impersonation **[unverified]**.
- Fallback if DI exchange fails entirely: GET `<service_url>?ticket=<ST>` and harvest cookie `JWT_WEB` (legacy cookie-auth path, see 1.5) [src session.py:134-160].

### Tokens produced, lifetimes, storage

- Result: `di_token` (access, a **JWT**), `di_refresh_token`, `di_client_id`. No OAuth1 token. [src session.py:242-244]
- Persisted as `garmin_tokens.json`: `{"di_token": ..., "di_refresh_token": ..., "di_client_id": ...}` (file 0600 in a 0700 dir) [src client/tokens.py:117-169].
- Lifetimes: **not stated anywhere in source or docs. [unverified]**. Client reads `exp` from the unverified JWT payload and refreshes when `now > exp - 900` (15 min margin) [src tokens.py:95-115]. Refresh-token lifetime unknown. A Rust implementation should do the same: base64url-decode JWT payload, read `exp`, `client_id`.

### Refresh [src client/session.py:246-279, 287-298; transport.py:210-241]

```
POST https://diauth.garmin.com/di-oauth2-service/oauth/token
Authorization: Basic base64("<di_client_id>:")
Accept: application/json
Content-Type: application/x-www-form-urlencoded
Cache-Control: no-cache
+ the same native headers as step 4

grant_type=refresh_token&client_id=<di_client_id>&refresh_token=<di_refresh_token>
```
Response: `access_token` (required), `refresh_token` (optional; kept if absent, so the refresh token **may rotate**; store it every time) [session.py:273-279].
Triggers: (a) before each API call if the token expires within 900 s; (b) once after an API 401, then the request is retried [transport.py:211-237]; (c) at load time from a token file [api/base.py ~ "Proactively refresh DI token"]. Non-2xx refresh -> `AuthenticationError`; then caller must do a full credential login (+ MFA). Refresh needs no cookies and no SSO. This is the part that will keep a headless forwarder running without MFA.

### 1.4 Widget (strategy 3) and 1.5 Portal (strategies 4/5), for completeness

- **Widget** (HTML form, impersonates "chrome"): GET `https://sso.garmin.com/sso/embed?id=gauth-widget&embedWidget=true&gauthHost=https://sso.garmin.com/sso`; GET `/sso/signin` (query `id, embedWidget, gauthHost=<embed>, service=<embed>, source=<embed>, redirectAfterAccountLoginUrl=<embed>, redirectAfterAccountCreationUrl=<embed>`), scrape CSRF with `name="_csrf"\s+value="(.+?)"`; 3-8 s sleep; POST `/sso/signin` form `username,password,embed=true,_csrf`; success = `<title>Success</title>` and ticket via `\?ticket=(ST-[^"&\s]+)`; MFA page = title contains "mfa" or "authentication application" plus inline JS vars (`customerGuid, mfaMethod, locale, clientId, codeSentTo`, regex at constants.py:103-105); MFA submit = POST `/sso/verifyMFA/loginEnterMfaCode` form `mfa-code, embed=true, _csrf, fromPage=setupEnterMfaCode` [src sso.py:604-819, constants.py:98-105]. Garmin has returned 427 on this page when blocking [CHANGELOG 0.3.20 / PR #477].
- **Portal**: `GET https://sso.garmin.com/portal/sso/en-US/sign-in?clientId=GarminConnect&service=https://connect.garmin.com/app`, sleep **10-20 s** "anti-WAF", then `POST /portal/api/login?clientId=GarminConnect&locale=en-US&service=...` with the same JSON body as the mobile flow and a random desktop browser UA + sec-ch-ua headers (from `ua-generator`) [src sso.py:839-899, constants.py:33-61]. Same `responseStatus` JSON.
- JWT_WEB path (legacy): API calls carry `Cookie: JWT_WEB=...`, `NK: NT`, `Origin/Referer connect.garmin.com`, `DI-Backend: connectapi.garmin.com` [src transport.py:136-147]. Not needed if DI exchange works.

## 2. Authenticated API requests [src client/transport.py:124-147, 210-281]

Base `https://connectapi.garmin.com/<path>`, headers = native headers from 1 step 4 + `Authorization: Bearer <di_token>` + `Accept: application/json`. A separate plain `requests.Session` with cookies disabled is used (no cookie jar for API) [src client/__init__.py "self._api_session" block; transport.py:224]. Default timeout 15 s. 204 -> empty object. Status >= 400 -> error `API Error <status> - <message|content|detailedImportResult.failures[0].messages[0]>`; 404 -> NotFound [transport.py:243-279].

IMPORTANT for TLS concerns: connectapi calls (including uploads) go through **plain `requests`/urllib3**, not curl_cffi, and the maintainer states this "has always worked for most users" [issue https://github.com/cyberjunky/python-garminconnect/issues/444]. The one report of 403 with a valid token turned out to be a `~/.netrc` entry overwriting `Authorization` (not Cloudflare) [issue #444, last comment].

## 3. FIT upload

### 3.1 `upload_activity` (default) [src api/activities.py:200-254]

```
POST https://connectapi.garmin.com/upload-service/upload
Authorization: Bearer <di_token>   + native headers (UA GCM-Android-5.23, X-Garmin-*, ...)
Content-Type: multipart/form-data; boundary=...
  part: name="file"; filename="<basename>.fit"   (Content-Type for the part not set explicitly; requests/urllib3 guesses from the filename)
```
URL constant: `self.garmin_connect_upload = "/upload-service/upload"` [src api/base.py:295]. Field name `file` [activities.py:241]. Extension check: `fit|gpx|tcx` only [activities.py:232-235, base.py:53-56]. Body of the part = raw file bytes.
Body-less extension path: NOT appended in this method.

### 3.2 `import_activity` (variant, "treated as import, not re-exported to Strava") [src api/activities.py:256-354]

```
POST https://connectapi.garmin.com/upload-service/upload/fit        (ext lowercase appended, no leading dot)
NK: NT
origin: https://sso.garmin.com
User-Agent: GCM-iOS-5.7.2.1                 (overrides the Android UA; the other native headers stay)
Authorization: Bearer <di_token>
multipart: file=(filename, bytes, "application/octet-stream")
```
[src activities.py:309-328]. Returns `response.json()` [activities.py:329-331].

### 3.3 Discrepancy to be aware of

The repo's own docs say the **web app** today sends multipart field `userfile` to `/upload-service/upload/.fit`, and that the library's `file` to `/upload-service/upload` "was confirmed to still work against a live account" [src docs/API_ENDPOINTS.md:315-330, row 330]. So three variants appear to work/exist: (a) `/upload` + `file`, (b) `/upload/fit` + `file`, (c) `/upload/.fit` + `userfile` (web). Which is best for a Rust port is a live-test question. garth-ng also uses `/upload-service/upload` + field `file` + part content type `application/octet-stream` [garth-ng/src/garth/http.py:402-422].

### 3.4 Response shapes

- **Success**: the client just returns the parsed JSON; **no success body is quoted anywhere in the repo's tests or docs** (no VCR cassette in the clone; `test_upload` only asserts truthiness, tests/test_garmin.py:240-256). So exact success status/body is **[unverified]**. From my background knowledge (not from source, treat as unverified): HTTP 201 (sometimes 202) with `{"detailedImportResult": {"uploadId":..., "fileName":..., "successes":[{"internalId":<activityId>,"externalId":...,"messages":...}], "failures":[]}}`; activity id then must be looked up (it may only appear after processing).
- **Duplicate**: **HTTP 409**, body `{"detailedImportResult": {"failures": [{"messages": [{"code": 202, "content": "Duplicate Activity."}]}]}}`; client message `API Error 409 - Duplicate Activity.` [src tests/test_garmin_unit.py:2357-2373, 2402-2416 (fixtures built by the maintainer, shape presumably recorded from real responses); transport.py:246-267; CHANGELOG.md 0.3.20 "Upload errors show Garmin's message text"]. `import_activity` wraps 409 as "Activity already exists (duplicate)" [activities.py:338-342]. For a forwarder: treat 409 + code 202 as idempotent success.
- **Other errors**: generic `API Error <status> - <detail>` where detail is `message` or `content` or the first failure message; demo mentions 413 as "file too large" [src transport.py:246-279; demo.py:2085-2110 (message only)]. Specific bodies for malformed/unsupported FIT: **unknown**.

### 3.5 401 / 429 / 5xx handling

- **401**: refresh the DI token once (`_refresh_session`), rewind the file handle, re-send once; if the file isn't seekable no retry [src transport.py:225-241]. If refresh fails the 401 raises.
- **429**: **no retry, no backoff** on API calls ("401, 429 and 4xx always fail fast") [src api/base.py:72-75]. Retries (default 3, exp. backoff 1-10 s) apply only to 5xx/network errors on `connectapi`, `connectwebproxy`, `download` per the docstring; whether upload (`client.post`) is covered is not stated **[unverified]**. For login, 429/427 trigger the next strategy and finally "All login strategies rate limited" [sso.py:243-248].
- **403**: ambiguous: either bot-block or genuinely rejected token; the client reports both possibilities [constants.py:80-90].

## 4. Cloudflare / bot protection

Evidence that bot handling is the dominant risk **at the SSO login step, not at connectapi**:
- Strategy order and doc comment: mobile+curl_cffi first ("TLS fingerprint rotation"), plain requests as fallback, portal with 10-20 s artificial delay because "Cloudflare flags rapid GET->POST sequences as bot-like" [src sso.py:1-13; constants.py:42-51].
- TLS impersonation profiles (curl_cffi names): mobile `safari_ios`, `safari`, `chrome120`; portal `safari, safari_ios, chrome120, edge101, chrome`; widget and DI-token POST `chrome` [src constants.py:53-61, sso.py:613, session.py:169]. "Different TLS fingerprints land in different Cloudflare rate-limit buckets" [sso.py:398-402].
- The maintainer on a user using only plain-requests mobile login: that strategy "is the one most likely to be refused... no guarantee", 429 is fingerprint based not IP, persisted > 24 h [issue https://github.com/cyberjunky/python-garminconnect/issues/453; PR #449; issue #444 "Related, smaller"].
- Header/cookie requirements: iOS Safari-like UA, `Origin`, `Referer` equal to the sign-in page URL, cookies from the sign-in page GET (SESSION, `__cf_bm`, `__cflb`), 2-5 s delay [src sso.py:556-598; PR #479]. The 403 branch is labelled "Cloudflare bot challenge" [sso.py:470-474].
- Random per-login UA only for the portal flow (`ua-generator`) [sso.py:80-89, 842].
- curl_cffi does not support `files=` multipart per the maintainer, so uploads stay on plain `requests`, evidence that connectapi + upload-service is not behind a fingerprint check **for most users** [issue #444 maintainer comment].

**Can plain `reqwest` work?** Split verdict:
- connectapi (upload, refresh via diauth (probably), data calls): **likely yes**, since the reference client does exactly this with plain urllib3 [src transport.py:224; issue #444]. Native headers must be sent verbatim.
- SSO login (`sso.garmin.com`): **uncertain/at risk**. The maintainer's own advice is that plain-Python TLS is the strategy most likely refused (#453) and the whole chain exists to rotate Safari/Chrome JA3/JA4 profiles. Options, all **[unverified]**: (1) do the interactive login rarely (once; MFA) from a machine where it works, e.g. via the python library or a real browser, and ship only `garmin_tokens.json` to the Rust forwarder, which then only refreshes + uploads; (2) use a browser-impersonating Rust client for the SSO steps only. Candidate crate: `wreq` (6.0.0-rc.31, updated 2026-10-30; successor of `rquest`, advertises TLS fingerprinting, per crates.io metadata; I did not inspect its source); (3) reqwest + rustls with hand-tuned settings is unlikely to match Safari iOS JA3/HTTP2 settings.
- Also: the DI token POST for refresh goes through curl_cffi(chrome) in the reference client when available; if diauth fingerprint-checks, a reqwest refresh might also fail. Not proven either way **[unverified]**. Worth a first live test of refresh with plain reqwest, since that determines whether the login must ever be done from Rust.

## 5. Issue state (last ~3 months, to 2026-11-19)

Source: GitHub API issue listing of cyberjunky/python-garminconnect updated since 2026-08-15 (14 issues, 0 open) and cyberfossa/garth-ng.

- **Maintained and currently working**: 0.3.20 (2026-10-23) is the latest; no commits other than README/dependabot since, no open login-breakage issues; the latest login fix was PR #479 (mobile login always 429 because the sign-in page wasn't loaded first, since April) and #477 (HTTP 427 treated as block) https://github.com/cyberjunky/python-garminconnect/pull/479 , https://github.com/cyberjunky/python-garminconnect/pull/477
- **Login blocking is fingerprint-based, not purely IP**: 429/427 even from never-used IPs; plain `requests` login is explicitly "not guaranteed" https://github.com/cyberjunky/python-garminconnect/issues/453 , PR #449 https://github.com/cyberjunky/python-garminconnect/pull/449
- **403 on connectapi with a valid DI token (#444)**: turned out to be a `~/.netrc` default entry replacing the Bearer header with Basic auth, not Cloudflare. Check your HTTP client doesn't add netrc/proxy auth. https://github.com/cyberjunky/python-garminconnect/issues/444
- **Past API-tier break (June 2026, #369)**: tokens from some strategies got `401 invalid_token "Token is not active"` from connectapi; fixed in later versions (strategy chain + `_verify_token`); shows Garmin changes this layer without notice and that different login strategies can yield tokens with different acceptance https://github.com/cyberjunky/python-garminconnect/issues/369 ; old garth-format tokens are explicitly unsupported (#439) https://github.com/cyberjunky/python-garminconnect/issues/439
- **garth-ng**: no human-filed issues in the last 3 months (only dependabot PRs #50-#55); last release 2.0.0-alpha.1 (2026-06-14), last commit 2026-06-26; implements the same DI flow with curl_cffi for everything including upload (`CurlMime`), so it is effectively dormant versus python-garminconnect https://github.com/cyberfossa/garth-ng/issues
- **Official alternative**: Garmin launched a Connect Developer Program "Personal API" (Oct 2026), but it is read-only and has only ~35 data points, no push/upload, so it can't replace upload (maintainer's conclusion) https://github.com/cyberjunky/python-garminconnect/issues/476 (see also https://github.com/garmin/connect-personal-api-python, linked by maintainer).
- Also relevant: Garmin Connect's API-returned FIT is a rewritten copy (blog link posted in #476), not relevant to upload.

## 6. Non-activity FIT files through `/upload-service/upload`?

- **Weight-scale FIT (file type 9) is accepted**: `add_body_composition()` builds a FIT with `FILE_TYPE = 9` and POSTs it to the same `/upload-service/upload`, multipart `file`, name `body_composition.fit` [src api/body.py:101-105 (upload block), fit.py:199, 271]. garth-ng does the same (PR #12 "add body composition FIT upload", `garth-ng/src/garth/data/weight.py:242`). So at least one non-activity type works.
- **Monitoring / wellness (type 32 monitoring_b, type 15 monitoring_a), sleep, HRV, stress FIT**: **unknown.** No source, issue or doc I read uploads or tests them; the python-garminconnect FIT encoder only writes weight files (fit.py). Web searches (Garmin forums) only surfaced "We don't support the file type you tried to import" errors for corrupt *activity* FIT files, nothing on monitoring files. Garmin's own docs on Import Data were not retrievable (support.garmin.com returned 403). Reading wellness data from the watch is normally via device sync (Garmin Connect Mobile/Express), not via this endpoint; I found no evidence this endpoint ingests it. Treat as "likely rejected or ignored, test with one throwaway file" **[unverified]**.
  - **Verified live 2026-11-22:** one Instinct 2 monitoring file (FIT 128/32, 962 bytes, 15 minutes of heart rate) sent through Forward's normal path (`POST /upload-service/upload`, field `file`) got a 2xx with no activity id. Within a minute Garmin Connect's heart-rate series for that day showed the file's 15 minutes. Sleep, metrics, HRV and device files are still untested.
- Related: the maintainer's own change log says Health Snapshot FIT can be *downloaded*, but that is download, not upload [src CHANGELOG 0.3.19].

## 7. Minimal Rust implementation sketch (from the above)

1. Persisted state: `{di_token, di_refresh_token, di_client_id}` (+ cookie jar only during a pending MFA login).
2. Per call: decode JWT `exp`; if `now > exp-900` -> refresh (section 1 "Refresh"); send native headers + Bearer.
3. Upload: POST multipart `file` to `https://connectapi.garmin.com/upload-service/upload`; handle 409 (`detailedImportResult.failures[0].messages[0].code == 202`) as "already there"; 401 -> refresh once and re-send; 429 -> back off yourself (the reference does none); 5xx -> bounded retry.
4. First-time login: Steps 1-4 above with a cookie-store client; implement MFA as two phases (return state, then verify). Decide whether to bring the login in-process (fingerprint risk) or bootstrap tokens out-of-band.
5. Cheap probes to run once you are allowed to touch Garmin (not done here): (a) does plain reqwest refresh against diauth work; (b) does plain reqwest `GET /userprofile-service/socialProfile` with Bearer + native headers return 200; (c) real success body of upload; (d) whether type-32 FIT is accepted.

## 8. Things I did not verify
- Any live behaviour (by instruction).
- Token lifetimes, upload success body/status, MFA code delivery trigger on JSON flows, whether diauth/sso block reqwest specifically.
- Whether the `wreq` crate really reproduces Safari iOS JA3/H2 fingerprints well enough for Garmin.
