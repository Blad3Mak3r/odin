# BepInEx updates

Odin records the installed BepInEx version for each instance and compares it
with the newest `denikson-BepInExPack_Valheim` release in Thunderstore's cached
package index. Legacy installations remain marked as installed with an unknown
version until they are updated. An unknown version is updateable; a local
version newer than Thunderstore's is never downgraded.

The dashboard exposes status and update actions in each instance's Mods tab,
plus a bulk action on the Instances page. Updates require an installed BepInEx
and a stopped instance. The `updating_bepinex` transition prevents concurrent
lifecycle or duplicate update operations for the full duration of the job.

Before changing an installation, Odin downloads and extracts the release into
staging and validates its BepInEx core layout. It copies only package-owned
files, preserving `BepInEx/config`, `BepInEx/plugins`, and unrelated local
files. Existing files that will be overwritten are backed up. A failed copy or
database write restores them, and the recorded version changes only after the
filesystem update succeeds.

Successful updates are persisted in job history as `bepinex_update` and emit a
`bepinex_updated` activity event and webhook containing the old (possibly
unknown) and new versions. A request that resolves to the installed or an older
release succeeds as a no-op and does not emit update activity.
