"""Check the built SBT interface against the non-transferable API baseline."""

import json
from pathlib import Path
import subprocess
import sys


EXPECTED = {
    "__constructor": [
        ("booking_contract", "address"),
        ("mint_authority", "address"),
        ("upgrade_authority", "address"),
    ],
    "get_config": [],
    "mint_for_booking": [("booking_id", "u64")],
    "get_credential": [("credential_id", "u64")],
    "get_credential_by_booking": [("booking_id", "u64")],
    "is_review_eligible": [("booking_id", "u64"), ("traveller", "address")],
    "mark_reviewed": [("booking_id", "u64"), ("review_hash", "bytesN<32>")],
    "get_review_status": [("booking_id", "u64")],
    "upgrade": [("new_wasm_hash", "bytesN<32>")],
    "set_upgrade_authority": [("new_upgrade_authority", "address")],
    "contract_version": [],
}

FORBIDDEN = (
    "transfer",
    "transfer_from",
    "approve",
    "set_approval_for_all",
    "burn",
    "revoke",
    "initialize",
    "set_owner",
)


def normalize_type(type_spec):
    if isinstance(type_spec, str):
        return type_spec
    if isinstance(type_spec, dict):
        if "bytes_n" in type_spec:
            return f"bytesN<{type_spec['bytes_n']['n']}>"
    raise SystemExit(f"Unsupported interface type encoding: {type_spec!r}")


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
    actual = {
        fn["name"]: [
            (arg["name"], normalize_type(arg["type"])) for arg in fn["inputs"]
        ]
        for fn in functions
    }
    if len(functions) != len(EXPECTED) or actual != EXPECTED:
        raise SystemExit(
            f"SBT interface changed; review ownership safety. Actual: {actual}"
        )
    for name in FORBIDDEN:
        if name in actual:
            raise SystemExit(f"Forbidden ownership-changing entrypoint present: {name}")
    print(
        "PASS: approved entrypoints include review + upgrade APIs; "
        "no transfer, approval, owner setter, or burn API; "
        "mint accepts only booking_id."
    )


if __name__ == "__main__":
    main()
