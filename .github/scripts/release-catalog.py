#!/usr/bin/env python3
"""Generate the local release catalog and verify its public counterpart.

The catalog records the verified Linux artifacts for website consumption.

Subcommands:
  catalog   build a versioned release catalog from the verified build outputs
  verify    compare the locally built catalog against the publicly served one
"""
import argparse
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

SCHEMA = 1
PRODUCT = "rootle"
DEFAULT_REPO = "rootledev/rootle"
ARTIFACT_RE = re.compile(r"^rootle-(?P<version>[^-]+)-(?P<target>.+)\.tar\.gz$")
ARTIFACT_TARGETS = frozenset({"x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"})


def utcnow() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_version(tag: str) -> tuple[int, int, int]:
    parts = tag.removeprefix("v").split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        raise ValueError(f"not a semver triple: {tag}")
    return int(parts[0]), int(parts[1]), int(parts[2])


def build_catalog(tag: str, dist: Path, base_url: str, published_at: str) -> dict:
    version = tag.removeprefix("v")
    artifacts = []
    for tarball in sorted(dist.glob("rootle-*.tar.gz")):
        match = ARTIFACT_RE.match(tarball.name)
        if not match:
            raise ValueError(f"unexpected artifact name: {tarball.name}")
        if match.group("version") != version:
            raise ValueError(f"{tarball.name} does not match tag {tag}")
        if match.group("target") not in ARTIFACT_TARGETS:
            raise ValueError(f"unsupported artifact target: {match.group('target')}")
        sidecar = tarball.with_name(tarball.name + ".sha256")
        if not sidecar.is_file():
            raise ValueError(f"missing checksum sidecar for {tarball.name}")
        recorded = sidecar.read_text(encoding="utf-8").split()[0]
        computed = sha256_of(tarball)
        if recorded != computed:
            raise ValueError(f"sidecar digest does not match {tarball.name}")
        artifacts.append({
            "target": match.group("target"),
            "name": tarball.name,
            "sha256": computed,
            "bytes": tarball.stat().st_size,
            "url": f"{base_url}/{tag}/{tarball.name}",
        })
    if {a["target"] for a in artifacts} != ARTIFACT_TARGETS:
        raise ValueError("both Linux release artifacts are required")
    return {
        "schema": SCHEMA,
        "product": PRODUCT,
        "version": version,
        "tag": tag,
        "published_at": published_at,
        "artifacts": artifacts,
    }


def facts(catalog: dict) -> dict:
    """The promotion-relevant facts: version, tag and artifact identities."""
    return {
        "version": catalog["version"],
        "tag": catalog["tag"],
        "artifacts": [
            {"target": a["target"], "name": a["name"], "sha256": a["sha256"],
             "bytes": a["bytes"], "url": a["url"]}
            for a in catalog["artifacts"]
        ],
    }


def cmd_catalog(args: argparse.Namespace) -> None:
    catalog = build_catalog(args.tag, args.dist, args.base_url,
                            args.published_at or utcnow())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    print(f"catalog: {len(catalog['artifacts'])} artifacts for {catalog['tag']}")


def cmd_verify(args: argparse.Namespace) -> None:
    local = facts(json.loads(args.local.read_text(encoding="utf-8")))
    public = facts(json.loads(args.public.read_text(encoding="utf-8")))
    if local != public:
        sys.exit("public catalog facts diverge from the build outputs:\n"
                 f"  local:  {json.dumps(local, sort_keys=True)}\n"
                 f"  public: {json.dumps(public, sort_keys=True)}")
    print(f"public catalog matches build outputs for {local['tag']}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)

    catalog = commands.add_parser("catalog", help="build the release catalog from dist/")
    catalog.add_argument("--tag", required=True, help="release tag, e.g. v0.33.0")
    catalog.add_argument("--dist", type=Path, default=Path("dist"))
    catalog.add_argument("--out", type=Path, default=Path("dist/catalog.json"))
    catalog.add_argument("--base-url",
                         default=f"https://github.com/{DEFAULT_REPO}/releases/download",
                         help="download base URL recorded in artifact urls")
    catalog.add_argument("--published-at", help="ISO timestamp; default: now (UTC)")
    catalog.set_defaults(run=cmd_catalog)

    verify = commands.add_parser("verify", help="compare local and publicly served catalogs")
    verify.add_argument("--local", type=Path, required=True)
    verify.add_argument("--public", type=Path, required=True)
    verify.set_defaults(run=cmd_verify)

    args = parser.parse_args()
    try:
        args.run(args)
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
