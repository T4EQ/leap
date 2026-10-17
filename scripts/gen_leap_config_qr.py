#!/usr/bin/env python3
"""Generates a QR code holding the LEAP configuration.

The payload is the JSON form of `LeapConfigQr` (see
`leap-provision-site/src/pages/leap_config.rs`).

Requires the `qrcode` python package (with PIL support).

Example:
    AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \\
        scripts/gen_leap_config_qr.py --bucket s3://my-bucket -o leap-config.png

An already built JSON payload can be encoded as is with `--json`:
    scripts/gen_leap_config_qr.py --json leap-config.json -o leap-config.png

When `json` is given the payload is written unmodified.
"""

import argparse
import json
import os
import sys

import qrcode

# Version of the `LeapConfigQr` payload.
QR_VERSION = 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generates a QR code holding the LEAP configuration.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )

    downloader = parser.add_argument_group("downloader")
    downloader.add_argument("--concurrent-downloads", type=int, default=4)
    downloader.add_argument(
        "--update-interval", default="1h", help="human readable duration, such as 1h or 30m"
    )
    downloader.add_argument(
        "--initial-backoff", default="1s", help="human readable duration, such as 1s or 5m"
    )
    downloader.add_argument("--backoff-factor", type=float, default=2.0)
    downloader.add_argument(
        "--max-backoff", default="1h", help="human readable duration, such as 1h or 30m"
    )

    s3 = parser.add_argument_group("s3")
    s3.add_argument("--bucket", help="bucket URI, such as s3://my-bucket")
    s3.add_argument(
        "--access-key-id",
        default=argparse.SUPPRESS,
        help="defaults to the AWS_ACCESS_KEY_ID environment variable",
    )
    s3.add_argument(
        "--secret-access-key",
        default=argparse.SUPPRESS,
        help="defaults to the AWS_SECRET_ACCESS_KEY environment variable",
    )
    s3.add_argument("--endpoint-url", help="defaults to AWS if not given")
    s3.add_argument("--region", help="defaults to us-east-1 if not given")
    s3.add_argument(
        "--force-path-style",
        action="store_true",
        help="use path-style access to the bucket (MinIO, Ceph, etc)",
    )

    parser.add_argument(
        "--json",
        metavar="FILE",
        help="encode this JSON file as is, ignoring the downloader and s3 arguments",
    )
    parser.add_argument(
        "-o", "--output", default="leap-config-qr.png", help="path of the generated PNG"
    )
    parser.add_argument(
        "--print", action="store_true", help="also print the QR code to the terminal"
    )

    args = parser.parse_args()

    # The file is used verbatim, so none of the other settings apply.
    if args.json:
        return args

    if not args.bucket:
        parser.error("--bucket is required")

    for name, env in (
        ("access_key_id", "AWS_ACCESS_KEY_ID"),
        ("secret_access_key", "AWS_SECRET_ACCESS_KEY"),
    ):
        if not hasattr(args, name):
            value = os.environ.get(env)
            if not value:
                parser.error(f"--{name.replace('_', '-')} or {env} is required")
            setattr(args, name, value)

    if args.concurrent_downloads < 1:
        parser.error("--concurrent-downloads must be a positive integer")
    if args.backoff_factor <= 1.0:
        parser.error("--backoff-factor must be greater than 1")

    return args


def leap_config_qr(args: argparse.Namespace) -> dict:
    """Builds the payload, mirroring the serde layout of `LeapConfigQr`."""
    s3_config = {
        "bucket": args.bucket,
        "access_key_id": args.access_key_id,
        "secret_access_key": args.secret_access_key,
        "force_path_style": args.force_path_style,
    }
    if args.endpoint_url:
        s3_config["endpoint_url"] = args.endpoint_url
    if args.region:
        s3_config["region"] = args.region

    return {
        "version": QR_VERSION,
        "downloader_config": {
            "concurrent_downloads": args.concurrent_downloads,
            "update_interval": args.update_interval,
            "retry_params": {
                "initial_backoff": args.initial_backoff,
                "backoff_factor": args.backoff_factor,
                "max_backoff": args.max_backoff,
            },
        },
        "s3_config": s3_config,
    }


def main() -> int:
    args = parse_args()

    if args.json:
        with open(args.json, "rb") as f:
            payload = f.read()
    else:
        payload = json.dumps(leap_config_qr(args), separators=(",", ":")).encode()

    qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_M)
    qr.add_data(payload)
    qr.make(fit=True)
    qr.make_image().save(args.output)

    if args.print:
        qr.print_ascii(invert=True)

    print(f"Wrote {args.output} (contains the S3 credentials, do not share it)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
