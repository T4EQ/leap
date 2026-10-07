# Provisioning configuration import

The content setup page accepts:

- An image containing one QR code whose payload is UTF-8 JSON (up to 10 MiB and 40 megapixels).
- A live camera scan, when the page runs in a secure browser context (trusted HTTPS or localhost).
  Device-local HTTP addresses do not permit live camera access. QR image import still works
  there. Camera permission is requested only when scanning starts.

Imports are decoded locally in Rust/WebAssembly. They populate the existing form only after
validation; they do not send settings to the device automatically. Choose **Apply settings**
to run the existing server-side S3 access check and save the configuration. **Advanced: enter
settings manually** starts collapsed and lets you inspect or edit every imported setting before
applying it. Apply uses the current edited fields, not a separate copy of the imported bundle.
You can also enter settings manually without importing a QR code.

## Preparing a configuration

Start with [example-config.json](example-config.json). Replace the example bucket, endpoint,
region and credentials with your deployment's values. `version` must be `1`. All downloader
fields are required; durations use human-readable strings such as `1h`, `30s` or `500ms`.
The optional S3 fields `endpoint_url`, `region` and `force_path_style` can be omitted.

For a QR code, encode the JSON text itself, not a URL, base64 string, or TOML document.
Use a local QR generator, compact the JSON to reduce density, and preserve the QR quiet zone.
For example, with `jq` and `qrencode` installed:

```sh
jq -c . config.json | tr -d '\n' | qrencode -l Q -s 8 -o config-qr.png
```

The bundle is distinct from the server's runtime configuration file. Treat both files and QR
images as secrets: do not submit real credentials to public QR-generator websites. Use narrowly
scoped S3 credentials. The frontend does not persist imported settings in browser storage.

## Checks

```sh
cargo test -p leap-provision-site
cargo check -p leap-provision-site --target wasm32-unknown-unknown
```
