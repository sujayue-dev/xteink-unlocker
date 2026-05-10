# Firmware Patches

Pre-patched CrossPoint firmware bins for cases the main catalog can't cover
directly. Each file here is a specific upstream build with targeted modifications
to work around known device or bootloader bugs.

Filenames are intentionally verbose so each file's purpose is self-documenting.

Users select these from Unlocker's Custom firmware browse flow. They are not
auto-listed in the Unlocker UI.

## Licensing

Binaries here are redistributions of MIT-licensed CrossPoint Reader builds with
community patches applied. See the upstream project for full license terms.

## Available patches

### `crosspoint-beta-Custom-Fonts-on-SD-v3-x3-efuseblk-patched-all-0b66.bin`

- **What:** CrossPoint beta commit `0b66`, "Custom Fonts on SD v3" branch, pre-#1786
- **Patched for:** X3 eFuse block validity workaround
- **Size:** 5,776,800 bytes
- **Built:** (date unknown, inherited)
- **Supported devices:** X3

### `crosspoint-91de6ac-1.2.0-escape.bin`

- **What:** CrossPoint master commit `91de6ac`, post-#1786
- **Patched for:** `esp_image_verify()` bug in CrossPoint 1.2.0 validator
- **Size:** 5,795,968 bytes
- **SHA256:** `4acfeb01602e0a2791fe847af6d84bb8ea8b4fe6fe0c76d0b488a7b6386f5842`
- **Built:** 2026-05-10
- **Supported devices:** X4 (confirmed), X3 (expected, not yet confirmed in this exact build)
- **When to use:** Users stuck on CrossPoint 1.2.0 where OTA fails at `esp_ota_end()`.
- **After install:** User lands on `1.2.0-dev+91de6ac` which includes #1786's raw-flash OTA path, so future updates work normally (including the Settings > System > SD Card Firmware Update menu for offline flashing).
- **Patch recipe:** bytes 144..255 of `esp_app_desc_t` zeroed in segment 0, XOR checksum and SHA256 trailer recomputed. Image header (bytes 0..23) untouched. Reproducible script: https://github.com/togotago/xteink-x4-escape/blob/master/patch.py
- **Remove when:** a CrossPoint stable release is tagged that incorporates #1786's fix (at which point users can OTA normally from 1.2.0 to that release without a patched bridge).
- **Related:** crosspoint-reader/crosspoint-reader#1918, crosspoint-reader/crosspoint-reader#1861, SoFriendly/xteink-unlocker#11

## Adding new patches

Include these in a PR:

1. The `.bin` file in this folder
2. An entry in this README following the pattern above (What, Patched for, Size, SHA256, Built, Supported devices, When to use, After install, Patch recipe, Remove when, Related)
3. Link to any upstream issue or writeup in the PR description

Use a descriptive filename that includes the source and purpose. Longer names are fine if they tell future maintainers what they're looking at.

Include a reproducibility note, whether that's a script, a linked gist, or precise step-by-step instructions. Future maintainers need to be able to rebuild from upstream as base firmware changes.
