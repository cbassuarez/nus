# Updates and Mercury

## Updates

The header has a download/update icon beside Search. It stays dim while no update
is ready and lights in the theme accent when a verified update is available. Its
tooltip and screen-reader label announce readiness. Clicking it opens Updates and,
when ready, the restart warning; it never installs without confirmation.

Settings → Updates shows the installed version, update status, automatic-check
toggle and **Check for updates**. Release builds check GitHub Releases at startup
and every six hours while running. Development builds and incognito do not check
automatically. A check sends a generic user agent and ordinary network metadata
such as the IP address to GitHub; it sends no usage events, profile identifiers,
file paths, text buffers or Mercury receipt. Users can disable checks.

Readiness uses semantic versions and the release channel: stable installs do not
take previews. Metadata must contain the expected target, exact filename/version,
size and SHA-256, matching GitHub's asset digest. Download starts only after the
user presses Update, reads the interruption warning and chooses
**Download & restart**. Save unsaved work first: shells and agents may be
interrupted, and automatic process resumption has not been extensively tested.

The installer stages beside the installed package, verifies the download,
extracts bounded paths, validates the reported version and checks the platform
signature. macOS checks codesigning and signing-team continuity, plus Gatekeeper
for notarized releases. Windows requires valid matching Authenticode publishers.
Linux currently uses HTTPS and matching SHA-256 metadata; it has no independent
publisher-signature guarantee. A compromised release account is outside that
checksum boundary. No updater requests elevated privileges.

A helper waits for nus to exit, retains the previous installation beside it,
swaps the staged package into place and restarts. A failed second rename restores
the previous package. In-app updates explicitly continue the existing profile;
manually redownloading a bundle retains the separate new-install/import flow.
Held processes and locked files may prevent an update, especially on Windows.
Interrupted downloads leave the running install intact. A retained previous
package allows manual recovery if the new app cannot launch; successful process
launch is not a health check or automatic rollback of a bad release.

The release pipeline embeds the tag as the binary version. Publishing must
continue to produce the `nus-release` manifest in the GitHub release body and
GitHub asset digests. Missing or mismatched metadata makes that package unavailable
to the updater. Native signed-package installation tests on every OS are required
before calling automatic installation production-validated.

## Mercury

**Settings → Look → App icon → Mercury** shows the silver icon and **Claim Mercury**
(**Replay Mercury** once earned). Mercury is not part of Welcome or onboarding,
and visiting Settings never claims it automatically. The profile card retains
**Used since**. Eligibility is local:
the window closes at whichever happens first, **2027-01-01 00:00:00 UTC**
or this installation first running **1.0.0 or later**. Version 1.0.0 itself
cannot claim. A local closure marker prevents downgrading from reopening claims.
The version trigger is this installation, not the global publication date. A successful claim writes its
time and original version before showing the silver-n ceremony, so interruption
of the animation does not lose the award. Reduced motion shows a static award
with the same text and Continue control. The ceremony can be replayed. Claim and replay share the standard nus
outlined rectangular button treatment and hard offset shadow.

The claim presentation uses the selected Tidal Mercury artwork with two beads
at the orbit tip. A white cyclorama, soft contact shadow and a procedural field of
twinkling stars surround the icon inside the modal only. A continuous ordered
dither pattern follows the pointer and dissolves over 2.3 seconds.
The icon's reflections flow along its strokes while the authored silhouette
stays fixed, continuously while the modal is open. The larger Settings preview
uses the same flow while visible. The existing claim fade-in is preserved. **Continue** and
Escape return to Settings; Enter or Space activates Continue. Reduced motion
presents a fully static scene immediately. Screen readers receive a modal tree
containing only the Continue action.

The saved receipt remains earned after the date or version window closes. It
changes the running app icon; it does not rewrite signed bundle resources. On
macOS, claim, replay and launch draw a new silver shell over the currently
displayed Dock icon. The base stays visible until the final part of the loop,
then retires as the coating completes. A connected liquid front is fed by a
mercury bead. The stems, arch and orbit fill along their natural paths; the two
orbital beads collect last. The Dock then settles on the approved still artwork
and its animation timer stops. Only the current construction frame is decoded;
the claim scene and Settings preview retain their flowing reflections. The OS owns the
bounce; no extra jump is animated. The closed app's Finder/Dock bundle icon
remains the distributed icon. Other desktop launchers keep the still icon.
The receipt belongs to the local profile and can travel through explicitly
configured encrypted profile sync. “Used since” is the profile creation date,
not a globally verified account-registration date.

There is no first-1,000 cap, worldwide user number, claim endpoint or usage
telemetry. Local clock/profile editing can change eligibility; that is an
intentional consequence of a local commemorative award, not enforceable scarcity.

References: [GitHub Releases API](https://docs.github.com/en/rest/releases/releases),
[GitHub asset digests](https://github.blog/changelog/2025-06-03-releases-now-expose-digests-for-release-assets/).

See [verification results and remaining native release gates](UPDATES_SECURITY_VERIFICATION.md).

## Multi-version continuity

The compatibility gate, profile generations, recovery action and release maintenance
policy are documented in [Multi-version support](MULTI_VERSION_SUPPORT.md). Those
checks extend the original package-only backup behavior described above.
