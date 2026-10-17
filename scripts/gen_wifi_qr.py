#!/usr/bin/env python3
"""Generates a QR code holding the settings of a WiFi network.

The payload uses the `WIFI:T:WPA;S:<ssid>;P:<password>;;` format.
See https://github.com/zxing/zxing/wiki/Barcode-Contents#wi-fi-network-config-android-ios-11
for details.

Requires the `qrcode` python package (with PIL support).

Example:
    WIFI_PASSWORD=... scripts/gen_wifi_qr.py --ssid "School Wi-Fi" -o wifi.png
"""

import argparse
import os
import sys

import qrcode

# Escaped characters
SPECIAL_CHARS = r'\;,":'


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generates a QR code holding the settings of a WiFi network.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )

    parser.add_argument("--ssid", required=True, help="name of the network")
    parser.add_argument(
        "--password",
        default=argparse.SUPPRESS,
        help="defaults to the WIFI_PASSWORD environment variable",
    )
    parser.add_argument(
        "--open", action="store_true", help="the network has no password"
    )
    parser.add_argument(
        "--hidden", action="store_true", help="the network does not broadcast its SSID"
    )
    parser.add_argument(
        "-o", "--output", default="wifi-qr.png", help="path of the generated PNG"
    )
    parser.add_argument(
        "--print", action="store_true", help="also print the QR code to the terminal"
    )

    args = parser.parse_args()

    if args.open:
        if hasattr(args, "password"):
            parser.error("--password can't be used with --open")
        args.password = None
    elif not hasattr(args, "password"):
        args.password = os.environ.get("WIFI_PASSWORD")
        if not args.password:
            parser.error("--password, WIFI_PASSWORD or --open is required")
    elif not args.password:
        parser.error("--password can't be empty, use --open for open networks")

    return args


def escape(value: str) -> str:
    return "".join(f"\\{c}" if c in SPECIAL_CHARS else c for c in value)


def wifi_qr(args: argparse.Namespace) -> str:
    fields = [("T", "nopass" if args.password is None else "WPA"), ("S", args.ssid)]
    if args.password is not None:
        fields.append(("P", args.password))
    if args.hidden:
        fields.append(("H", "true"))

    return "WIFI:" + "".join(f"{k}:{escape(v)};" for k, v in fields) + ";"


def main() -> int:
    args = parse_args()

    payload = wifi_qr(args).encode()

    qr = qrcode.QRCode(error_correction=qrcode.constants.ERROR_CORRECT_M)
    qr.add_data(payload)
    qr.make(fit=True)
    qr.make_image().save(args.output)

    if args.print:
        qr.print_ascii(invert=True)

    notice = "" if args.password is None else " (contains the WiFi password, do not share it)"
    print(f"Wrote {args.output}{notice}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
