"""Compute the CSSwitch static-model-catalog fingerprint for a catalog JSON file.

The gateway refuses a catalog whose `catalog_fp` does not match its own
`wire_catalog_fingerprint`, so fixtures must compute it the same way:
SHA-256 over a length-prefixed canonical byte sequence. This keeps
`test/win-e2e.ps1` honest when the fixture catalog changes.

usage: catalog_fp.py <catalog.json>
"""

import hashlib
import json
import struct
import sys


def fingerprint(catalog: dict) -> str:
    digest = hashlib.sha256()
    digest.update(b"csswitch-static-catalog-fp-v1\0")
    digest.update(struct.pack(">I", catalog["schema_version"]))

    def text(value: str) -> None:
        raw = value.encode("utf-8")
        digest.update(struct.pack(">I", len(raw)))
        digest.update(raw)

    text(catalog["adapter"])
    text(catalog["default_selector_id"])
    routes = catalog["routes"]
    digest.update(struct.pack(">I", len(routes)))
    for route in routes:
        text(route["selector_id"])
        text(route["display_name"])
        text(route["upstream_model"])
        supports_tools = route.get("supports_tools")
        digest.update(bytes([0 if supports_tools is None else (1 if not supports_tools else 2)]))
        capabilities = route.get("capabilities") or {}
        digest.update(bytes([]))
        text(capabilities.get("reasoning_round_trip", "none"))
        for key in ("forced_tool_choice", "structured_output", "vision"):
            value = capabilities.get(key)
            digest.update(bytes([0 if value is None else (1 if not value else 2)]))
    roles = catalog["role_bindings"]
    for key in ("sonnet", "opus", "haiku", "fable"):
        text(roles[key])
    aliases = catalog.get("legacy_aliases") or []
    digest.update(struct.pack(">I", len(aliases)))
    for alias in aliases:
        text(alias["alias"])
        text(alias["selector_id"])
    return digest.hexdigest()


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: catalog_fp.py <catalog.json>", file=sys.stderr)
        return 2
    with open(sys.argv[1], "r", encoding="utf-8") as handle:
        catalog = json.load(handle)
    print(fingerprint(catalog))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
