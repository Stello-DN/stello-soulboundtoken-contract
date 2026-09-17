"""Check the built SBT interface against the non-transferable API baseline."""

import json
from pathlib import Path
import subprocess
import sys


EXPECTED = {
    "__constructor": [("booking_contract", "address"), ("mint_authority", "address")],
    "get_config": [],
    "mint_for_booking": [("booking_id", "u64")],
    "get_credential": [("credential_id", "u64")],
    "get_credential_by_booking": [("booking_id", "u64")],
    "is_review_eligible": [("booking_id", "u64"), ("traveller", "address")],
}


def main():
    wasm = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(
        "target/wasm32v1-none/release/stello_sbt_contract.wasm"
    )
    result = subprocess.run(
        ["stellar", "contract", "info", "interface", "--wasm", str(wasm),
         "--output", "json"],
        check=True, capture_output=True, text=True,
    )
    functions = [entry["function_v0"] for entry in json.loads(result.stdout)
                 if "function_v0" in entry]
    actual = {fn["name"]: [(arg["name"], arg["type"]) for arg in fn["inputs"]]
              for fn in functions}
    if len(functions) != len(EXPECTED) or actual != EXPECTED:
        raise SystemExit(f"SBT interface changed; review ownership safety. Actual: {actual}")
    print("PASS: six approved entrypoints; no transfer, approval, owner setter, "
          "burn or upgrade API; mint accepts only booking_id.")


if __name__ == "__main__":
    main()
