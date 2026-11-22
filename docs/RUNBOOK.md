# Runbook

How to run a Nimrag Connect server day to day: install it, upgrade it, back it up, get the data back
out, and deal with the watch. The names below are examples: the user is `alice`, the server is
`nimrag.example.ts.net`, the commands run in the folder with `docker-compose.yml`.

Not everything here has been tried on a real server. Each section says what was tested. The
backup, restore, `reparse` and export commands are covered by automated tests that run the real
binary (`server/tests/backup.rs`, `server/tests/export.rs`). The Docker forms of the same commands
were not run in the build session.

## Contents

1. [Install](#install)
2. [Upgrade](#upgrade)
3. [Backups](#backups)
4. [Restore](#restore)
5. [Check that your backups restore](#check-that-your-backups-restore)
6. [Export your data](#export-your-data)
7. [Re-pair the watch](#re-pair-the-watch)
8. [Firmware updates over USB](#firmware-updates-over-usb)
9. [Map tiles](#map-tiles)
10. [Weather for the watch](#weather-for-the-watch)
11. [GPS assist (satellite data for the watch)](#gps-assist-satellite-data-for-the-watch)
12. [Troubleshooting](#troubleshooting)

## Install

You need a machine that stays on (a small server, a NAS, a Raspberry Pi 4 or better) with Docker and
Docker Compose, and a way to reach it over HTTPS from your phone.

```sh
git clone https://github.com/AngelFreak/Nimrag-Connect.git
cd Nimrag-Connect
docker compose up -d --build
```

The first build takes several minutes (it builds the web app and the server). The server listens on
`127.0.0.1:8096` on that machine only. Data lives in the Docker volume `nimrag_data`.

**HTTPS.** Login cookies are `Secure`, so browsers and the app need HTTPS. Two common ways:

* A VPN or tailnet that gives the machine a name with a certificate, for example
  `https://nimrag.example.ts.net`. With Tailscale: `tailscale serve --bg 8096`.
* A reverse proxy (Caddy, nginx) with a certificate, forwarding to `127.0.0.1:8096`. Start the server
  with `--trusted-proxy` for the proxy's address, so the login limiter sees the real client address
  (see "Owner API" in [`server/README.md`](../server/README.md)).

Do not publish port 8096 to the open internet without TLS. `--insecure-cookies` exists only for a
plain-http home network.

**First run.**

1. Open the address in a browser. `/setup` creates the **admin** account: a username, a password of
   at least 10 characters, and a two-step code from an authenticator app. The admin sets the server
   up and has no data of its own.
2. In **Admin**, create a user, for example `alice`. The server shows a temporary password once.
   Hand it over in person.
3. The user signs in with that password, scans the QR code with an authenticator app, and chooses a
   password.
4. On the phone, install the app and sign in with the server address, username, password and code.
   The app gets its own upload credential; nobody types or copies tokens. Later unlocks use the
   fingerprint or the password.
5. In the app's **Watch** tab, pair the watch (see [Re-pair the watch](#re-pair-the-watch)).

**Lost password or authenticator.** On the server, with the server stopped or running:

```sh
printf '%s\n' "a new password of ten or more characters" |
  docker compose exec -T server nimrag-server user set-password --username alice
docker compose exec server nimrag-server user reset-2fa --username alice
```

The user enrols a new authenticator at the next login. (`user` and `token` commands do not need the
server to be stopped.)

**Your older history.** `import-export` (a Garmin data export) and `import-connect` (Garmin Connect
JSON) are described in [`server/README.md`](../server/README.md). They take the data directory's
lock, so stop the server first: `docker compose stop server`, run the command with
`docker compose run --rm --no-deps server import-export --user alice /path/inside/container`
(mount the archive's folder with `-v`), then `docker compose start server`.

## Upgrade

1. Take a backup first (see [Backups](#backups)). The server migrates its database when it starts,
   and there is no migration back.
2. Get the new version and rebuild:

   ```sh
   git pull
   docker compose up -d --build
   docker compose logs --tail 50 server
   ```

3. Open the web app and check that the data is there. The log shows no errors.
4. Update the Android app by installing the new APK over the old one (Releases page). An update
   installed over the old app keeps its data, so the sign-in and the paired watch stay.

To go back to an older version: stop the server, [restore](#restore) the backup you took in step 1
into a fresh volume, check out the old version, and start it. An older server cannot open a database
a newer one has migrated.

## Backups

A backup holds the database (accounts, tokens, who owns which file, settings, every derived table)
and the file store. Both are needed: the files alone lose the owners, the database alone loses the
originals. Details are in "Backups" in [`server/README.md`](../server/README.md).

**Scheduled.** Put a folder for backups on a different disk than the data, give the container's user
(uid 10001) write access, and switch it on in `docker-compose.yml` (the lines are there, commented):

```sh
mkdir backups
sudo chown 10001 backups
```

```yaml
    volumes:
      - nimrag_data:/data
      - ./backups:/backups
    environment:
      NIMRAG_BACKUP_DIR: /backups
      NIMRAG_BACKUP_EVERY: 1d      # 30m, 6h, 1d ...
      NIMRAG_BACKUP_KEEP: "7"
```

Then `docker compose up -d`. The log shows `backup done` with the file counts after each one. A failed
backup logs a warning and the server keeps running; look for `backup failed` in
`docker compose logs server`.

Each backup is a folder `nimrag-backup-<UTC time>` with `nimrag.db`, `files/` and `backup.json`.
Unchanged files are hard-linked to the previous backup, so a daily backup costs little more than
the new files. Only the newest 7 are kept.

**One now** (works while the server runs):

```sh
docker compose exec server nimrag-server backup --out /backups
```

Without Docker: `nimrag-server backup --data-dir ./data --out /mnt/backupdisk/nimrag`.

**Off the machine.** A backup on the same machine does not survive that machine. Copy the backup
folder somewhere else (another disk, a NAS, `rsync` to another host). Copy with a tool that keeps
hard links (`rsync -aH`), or the copy grows to the full size of every backup.

```sh
rsync -aH backups/ backuphost:/srv/nimrag-backups/
```

## Restore

Restore always goes into a new, empty data directory. It refuses one that has anything in it, so it
cannot damage a running server.

```sh
nimrag-server restore --from BACKUPS/nimrag-backup-<time> --data-dir NEWDIR
```

It copies the backup, checks the database, and empties `NEWDIR` again if anything fails.

**Docker, after losing the data volume (or to go back).** The old volume must go first, because
`/data` has to be empty:

```sh
docker compose down
docker volume rm nimrag_data        # only when it is lost, or you have a backup you trust
docker compose run --rm --no-deps -v ./backups:/backups:ro server \
  restore --from /backups/nimrag-backup-<time> --data-dir /data
docker compose up -d
```

Compose creates the empty volume again with the right owner. Open the web app: the data, the users
and the signed-in phones are all there, and the app keeps uploading with the token it has.

**Without Docker:**

```sh
nimrag-server restore --from /mnt/backupdisk/nimrag/nimrag-backup-<time> --data-dir ./data-new
nimrag-server serve --data-dir ./data-new
```

Notes:

* A file that arrived between the database snapshot and the file copy has no database row. After the
  restore, run `nimrag-server rebuild-ledger` to add it (it is then unassigned, and an admin assigns
  it in the web app). With a daily backup this is at most a sync's worth of files.
* Tokens and sessions are in the database, so phones keep uploading with the token they have.
* The restored data is as old as the backup. The watch still has what it has not yet handed over
  (the watch keeps files until a sync archives them), and the next sync uploads them.

**If only the database is lost but `files/` survived.** `rebuild-ledger` and `reparse` rebuild the
file records and every derived table from the files, but not the accounts, tokens or file owners.
Use a backup for those.

## Check that your backups restore

A backup nobody has restored is a hope. Do this once after switching backups on, and again after
every upgrade of the server. It restores the newest backup into a scratch directory, rebuilds the
derived tables from the files, and shows that the result is the data you have. It touches nothing
that runs.

On the host (Docker, nothing persists; the scratch directory is inside the throw-away container):

```sh
NEWEST=$(ls backups | grep '^nimrag-backup-' | sort | tail -n 1)
docker compose run --rm --no-deps -v ./backups:/backups:ro --entrypoint sh server -c "
  set -e
  nimrag-server restore --from /backups/$NEWEST --data-dir /tmp/check
  nimrag-server user list --data-dir /tmp/check
  nimrag-server reparse --data-dir /tmp/check
"
```

Without Docker, the same in plain commands:

```sh
nimrag-server restore --from BACKUPS/nimrag-backup-<time> --data-dir /tmp/nimrag-check
nimrag-server reparse --data-dir /tmp/nimrag-check
```

What to look for:

* `restore` ends without an error and prints `restored N files ... and the database`.
* `user list` shows your users.
* `reparse` prints `reparsed: N files` and exits with 0. A non-zero exit and a `file_parse` error
  name a file that is missing or damaged in the backup.
* For a strict check, compare the restored database with the live one. Stop the server first,
  because `reparse` takes the data directory's lock (`docker compose stop server`), run
  `nimrag-server reparse` on the live data dir as well, and compare the row counts of the derived
  tables in both (`sqlite3 DB 'select count(*) from activities'` and so on). The tables must be the
  same. `server/tests/backup.rs` does this comparison for every table on every test run
  (`real_binary_serve_backs_up_backup_runs_beside_it_and_restore_then_reparse_matches`).

Remove the scratch directory when you are done (`rm -r /tmp/nimrag-check`).

## Export your data

Each user can take all their own data out, at any time, in open formats.

* **Web:** Profile, **Download my data**. In the Android app the same row hands the download to the phone's download manager (not yet
  tried on a real phone). An admin has no data and gets nothing.
* **Command line**, for any user, while the server runs:

  ```sh
  docker compose exec server nimrag-server export --user alice --out /tmp/alice.zip
  docker compose cp server:/tmp/alice.zip .
  ```

  Without Docker: `nimrag-server export --data-dir ./data --user alice --out alice.zip`.

The zip holds `originals/` (the FIT files exactly as they came from the watch), `csv/` (one CSV
file per kind of data: activities, sleep, heart rate, positions and so on, with a header row, times
in ISO 8601 in the user's time zone plus a UTC epoch column) and a `README.txt`. See "Export" in
[`server/README.md`](../server/README.md) for the exact layout.

The export is the user's data, not a server backup: it has no accounts, tokens or settings of other
users. Use [Backups](#backups) for that.

## Re-pair the watch

The app talks to the watch over Bluetooth on its own, without the Garmin Connect app. You need to
pair again when the Watch tab shows "The phone or the watch forgot the pairing", after a new phone,
after a watch reset, or after Bluetooth was cleared on either side.

1. On the phone, open the **Watch** tab. If the old watch is still listed, choose **Forget watch**
   (stored files stay on the phone and still upload).
2. In the phone's Bluetooth settings, forget the watch if it is still listed there.
3. On the watch, open the settings (hold MENU) and choose **Pair phone**.
4. In the app, follow the setup steps on the Watch tab. Android asks for permission to find nearby
   devices and shows a dialog to associate the watch.
5. Wait for the first sync. The status shows the time of the last one.

If the watch is also paired with the Garmin Connect app, the two compete: only one app should be the
watch's phone. To switch for good, remove the watch in Garmin Connect (Devices), forget it in the
phone's Bluetooth settings, then pair it in Nimrag Connect. Nimrag Connect must be the only app that syncs the watch (keep Garmin Connect's "Nearby devices" permission off). To keep your Garmin Connect account up to date as well, turn on **Send to Garmin Connect** in your Nimrag profile (Forward): the server sends your activities, or everything from the watch, to your own Garmin account. The app's earlier "Alongside" mode, where both apps synced the watch, was removed in 0.8.0: Garmin Connect never archived the files Nimrag copied, and on 2026-11-22 the watch stopped recording heart rate and stress once about 75 of them piled up.

Pairing a watch that has no old pairing at all has not been tried yet on a real watch: the live
runs reused an existing Garmin Connect pairing. If a fresh pairing fails, the app's sync history
has a log you can share; please report it.

Syncing is every 15 minutes by default. A phone that is switched off or out of range catches up at
the next sync. The watch keeps what it has not handed over.

## Firmware updates over USB

Garmin delivers watch firmware through Garmin Express (Windows or macOS) or through the Garmin
Connect app. Nimrag Connect does not install firmware. To update over USB:

1. Plug the watch into a computer with Garmin Express and let it install the update. Garmin Express
   needs a Garmin account for this and for downloading the files.
2. Before you start, let the watch sync once (open the Watch tab, **Sync now**) so nothing is left on
   the watch that only a sync would collect.
3. After the update, wait for the watch to restart and reconnect. The Watch tab reconnects by itself
   in a few minutes.

Nimrag Connect is expected to keep working after an update: the pairing lives in Bluetooth, not in
the firmware. This has not been checked across a real update yet. If the Watch tab shows that the
pairing is lost, follow [Re-pair the watch](#re-pair-the-watch). A new firmware version that changes
how the watch syncs would show up as files that no longer arrive or no longer parse; a file that
fails to parse is recorded as an error and kept, and `reparse` retries it after a server update.

You can also copy the watch's files over USB without Garmin software: the watch shows up as a USB
drive. Upload any FIT file from it with a token (`nimrag-server token create --name usb --user
alice`) as in "Running the server from source" in the [README](../README.md); files that are already
known are ignored.

## Map tiles

Maps are optional. Without a tile file, routes are drawn without a map. How to cut a PMTiles
extract around where you train, mount it, and switch it on is in "Map tiles" in
[`server/README.md`](../server/README.md). In Docker, mount the folder read-only and set
`NIMRAG_TILES_DIR` (the lines are in `docker-compose.yml`). A new file replaces the old one without a
restart. The map data is OpenStreetMap; the web app shows the attribution.

## Weather for the watch

The phone feature "Weather" (off by default, switched on per phone in the app) asks this server for
the forecast, and the server asks [Open-Meteo](https://open-meteo.com/) (free, no key, CC BY 4.0).
That is the only thing the server fetches from the internet for the watch. What leaves your server:
a request to `api.open-meteo.com` with a position rounded to 0.1 degree (about 10 km) and nothing
that says who you are. The server keeps no position, writes none to its log, and caches each answer
for 30 minutes per 10 km cell, in memory only.

To switch it off for the whole server, set `NIMRAG_WEATHER: "off"` (or start with `--weather off`);
`/api/device/weather` then answers `503` and Open-Meteo is never contacted. `NIMRAG_WEATHER_URL`
(`--weather-url`) points the server at another Open-Meteo compatible address, for example your own.
The server needs the locally generated FIT profile to write the answer, so the default build and the
Docker image answer `503` ("without the FIT profile") and the phone then shows no weather. Details
are in "Weather" in [`server/README.md`](../server/README.md).

## GPS assist (satellite data for the watch)

With the "GPS assist" switch on the phone's Phone features page, the watch gets satellite orbit
data and finds GPS faster. The phone asks your server, and your server downloads the file from
Garmin (`api.gcs.garmin.com`, no login, about 66 KB, cached for 4 hours). This is the **only**
request Nimrag ever makes to Garmin, and nothing about you is sent with it (no token, no unit id,
no position). The phone switch is off by default.

To make the server refuse to contact Garmin at all, set `NIMRAG_GPS_ASSIST=off` (or start with
`serve --no-gps-assist`) and restart: `/api/device/gps-assist` then answers 404 and the phone's
switch simply finds nothing. In Docker add `NIMRAG_GPS_ASSIST: "off"` under `environment:`. The
downloaded files sit in `<data dir>/gps-assist/`; the folder is a cache, is not backed up, and can be
deleted. Details in "GPS assist" in [`server/README.md`](../server/README.md).

## Troubleshooting

**The log.** `docker compose logs --tail 100 server` (add `-f` to follow). `RUST_LOG=debug` and
`--log-json` give more.

**"data dir ... in use".** A command that writes (`reparse`, `rebuild-ledger`, the imports) needs the
server stopped, because only one writer may hold the data directory. `docker compose stop server`,
run it, `docker compose start server`. `backup`, `export`, `user` and `token` do not need that.

**Can't sign in.** The message is the same for a wrong username, password or code, on purpose. After
several failures the server makes you wait, per address and per username. Check the phone's clock
(two-step codes depend on it). Reset the password or the authenticator with the commands under
[Install](#install).

**The page loads but signing in loops or the cookie is rejected.** The cookie is `Secure`: use the
HTTPS address, not `http://`. Behind a proxy, forward the `Host` header and keep the proxy's address
in `--trusted-proxy`.

**The app says the server no longer accepts the phone.** Its token was signed out (Profile, Signed-in
phones, or a password reset). Sign in again in the app.

**A backup failed.** `backup failed` in the log names the cause: a full disk, a permission problem
on the backup folder (it must be writable by uid 10001), or a file that could not be read. The old
backups are kept, and the next try is within an hour.

**Backups use too much space.** Lower `NIMRAG_BACKUP_KEEP`, or move the folder to a bigger disk.
Copying the folder without `-H` breaks the hard links and multiplies its size.

**The watch does not sync.** The Watch tab says why in its banner. The usual causes are Bluetooth
off, the "Nearby devices" permission removed, battery optimisation killing the app (allow it to run in
the background), the watch out of range, or a lost pairing ([Re-pair the watch](#re-pair-the-watch)).

**Files arrive but a page is empty.** Derived data is rebuilt from the files. Stop the server, run
`nimrag-server reparse`, start it again. If a file is reported as a `file_parse` error, keep it: a
later version may read it.

**Disk full.** The file store only grows (the originals are the source of truth). Check
`docker system df -v` and the size of `nimrag_data`. Move backups off the machine first.

**An export fails.** At most two run at once (HTTP 429, try again in a minute). A very large export
needs a client that keeps reading; a download that stalls for two minutes is ended.
