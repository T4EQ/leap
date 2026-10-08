#!/usr/bin/env python3
"""Create a provisioning QR image from a JSON configuration file."""

import argparse
import json
from pathlib import Path

import qrcode
from qrcode.exceptions import DataOverflowError


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="JSON configuration file")
    parser.add_argument(
        "-o", "--output", type=Path, default=Path("config-qr.png"),
        help="Output PNG path (default: config-qr.png)",
    )
    args = parser.parse_args()

    try:
        config = json.loads(args.input.read_text(encoding="utf-8"))
        # Compact JSON keeps the QR smaller. Encode the JSON itself—not base64.
        payload = json.dumps(
            config, separators=(",", ":"), ensure_ascii=False
        ).encode("utf-8")

        qr = qrcode.QRCode(
            error_correction=qrcode.constants.ERROR_CORRECT_Q,
            box_size=10,
            border=4,  # Required white quiet zone around the code.
        )
        qr.add_data(payload)
        qr.make(fit=True)
        qr.make_image(
            fill_color="black", back_color="white"
        ).save(args.output, format="PNG")
    except json.JSONDecodeError:
        parser.exit(1, "Error: the input file is not valid JSON.\n")
    except DataOverflowError:
        parser.exit(1, "Error: the configuration is too large for one QR code.\n")
    except OSError:
        parser.exit(1, "Error: could not read the input or write the image.\n")

    print(f"Saved {args.output} ({len(payload)} payload bytes).")


if __name__ == "__main__":
    main()
