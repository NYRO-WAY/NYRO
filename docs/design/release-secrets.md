# Release Secrets (Desktop)

Nyro follows an updater-first release model (similar to Antigravity-Manager):
- Enable Tauri updater signature verification.
- Do not require macOS notarization or Windows code signing for now.

## Required (Tauri updater signing)

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

These two secrets are required to generate updater signatures (`.sig`) and publish `latest.json`.

## Optional (platform trust, not required for updater)

### macOS signing and notarization

- `APPLE_CERTIFICATE` (base64-encoded `.p12`)
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_APP_SPECIFIC_PASSWORD`
- `APPLE_TEAM_ID`

### Windows code signing

- `WINDOWS_CERTIFICATE` (base64-encoded `.pfx`/`.p12`)
- `WINDOWS_CERTIFICATE_PASSWORD`

Current release workflow does not require the optional platform-signing secrets.

## One-time setup commands

Generate updater keypair locally:

```bash
cargo tauri signer generate --write-keys ~/.tauri/nyro-updater.key
```

Then:

1. Copy the generated public key text into `src-tauri/tauri.conf.json` -> `plugins.updater.pubkey`.
2. Copy the private key file content into GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`.
3. Set GitHub Secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` to the password used during key generation.
4. Keep endpoint as:
   - `https://github.com/shuaijinchao/nyro/releases/latest/download/latest.json`

## RC validation checklist

1. Create and push an RC tag:
   ```bash
   git tag v0.1.0-rc1
   git push origin v0.1.0-rc1
   ```
2. Wait for `release-desktop` workflow to complete.
3. Confirm GitHub Release contains:
   - desktop installers
   - matching `.sig` files
   - `latest.json`
   - `SHA256SUMS.txt`
4. Install an older desktop build, then validate in-app update detects and upgrades to RC.
