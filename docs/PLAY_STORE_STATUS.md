# Google Play Store Status

Snapshot of the Play Console state for `com.cleanscope.app` and what stands
between internal testing and production. The live, machine-readable version is
`just play-status` (JSON) or the weekly `Play Store Status` workflow
(`.github/workflows/play-status.yml`, job summary + `play-status` artifact).

## Snapshot — 2026-10-27 (read from Play Console)

| Item | State |
|---|---|
| Account | Personal account `8619808928869873960` (12-tester closed-test rule applies) |
| App status | Draft app, temporary name `com.cleanscope.app (unreviewed)` |
| Production | Inactive; production access not yet applied for |
| Internal testing | Active, latest release **0.3.0** (GitHub is at v0.5.0) |
| Closed / open testing | No releases; 0 testers opted in |
| App content declarations | 10 of 10 completed (last edited 2026-03-20/22); none need attention |
| Store settings | Category Tools, tag Camera; contact email set; **website empty** |
| Default store listing | Text complete (en-GB); **app icon, feature graphic and phone screenshots missing** |
| Policy status | Nothing to show until the app has been reviewed |
| Android developer verification | Package name registered (2026-03-21), 1 key |
| Users and permissions | Owner only; the CI service account is **not invited** |

## Path to production

1. **Invite the CI service account** (`github-play-deploy@cleanscope-play.iam.gserviceaccount.com`)
   in Play Console → Users and permissions with *View app information*,
   *Release apps to testing tracks* and *Manage store presence* for CleanScope
   (`just play-console-setup`). Until this is done every upload fails with
   `The caller does not have permission` — the v0.4.0 and v0.5.0 publish jobs
   and the listing-sync job all failed this way, which is why the internal
   track is still on 0.3.0.
2. **Complete the Store Listing graphics**: 512×512 app icon, 1024×500 feature
   graphic, 2–8 phone screenshots. This is the last open item in the dashboard's
   "Finish setting up your app" (10 of 11 complete).
3. **Run a closed test** with at least 12 opted-in testers for at least 14
   consecutive days (required for personal accounts created after 2023-11-13).
4. **Apply for production access** from the dashboard and answer the
   closed-test questionnaire, then send the production release for review.

## Settings to review

- **Listing language mismatch** — Play's default listing is `en-GB`, while
  `fastlane/metadata/android/` only has `en-US`. Once the sync job has
  permission it will create a separate `en-US` listing instead of updating the
  default one. Either rename the directory to `en-GB` or switch Play's default
  language to English (United States).
- **Contact website** is empty; `https://laurigates.github.io/CleanScope/`
  (which already hosts the privacy policy) is the obvious value.
- **`INTERNET` permission** is declared in the manifest. The app is
  privacy-focused and the Data safety form should match; check whether release
  builds need it at all.
- **`r0adkll/upload-google-play@v1`** runs on Node 20 and its `track` input is
  deprecated in favour of `tracks` (both reported as annotations on the failed
  publish runs).

## Policy deadlines (2026-10-27)

From the [policy deadlines table](https://support.google.com/googleplay/android-developer/table/12921780?hl=en-GB):

| Deadline | Policy | Applies to CleanScope? |
|---|---|---|
| 2026-08-26 | Families / child endangerment / age-restricted — anonymous chat apps | No (not a chat app) |
| 2026-09-30 | Play Console requirements — register package names | Done (registered 2026-03-21) |
| 2027-01-27 | Contacts permission policy (Contact Picker) | No (no contacts permission) |
| 2027-01-27 | Location permission (location button) | No (no location permission) |
| 2027-01-27 | SMS / call log — phone-call verification removed | No |
| 2027-01-27 | Foreground service — geofencing removed | No (no foreground service) |

Nothing needs action. `targetSdk = 36` is current.

## What the API can and cannot see

The Android Publisher API v3 exposes tracks and release lifecycle states
(`applications.tracks.releases.list`), app details, store listing text and
testers (via a throwaway edit that is deleted, never committed), and users.
It has **no endpoint** for the dashboard checklist, Policy status, App content
declarations, the closed-test tester count or the Inbox, so those stay manual
Play Console checks. `scripts/play_status.py` lists them under
`console_only_checks` in its output. Policy deadlines come from parsing the
public help-centre table, flagged when an entry mentions a permission in
`AndroidManifest.xml` or applies to all apps.
