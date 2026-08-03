#!/usr/bin/env python3
"""Discover registry tool executables in disposable containers.

Each tool gets a fresh container and tmpfs-backed mise home. The only artifact
crossing the sandbox boundary is captured stdout/stderr; no host directory is
mounted writable into the container.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
IMAGE_RECIPE_VERSION = 2
MARKERS = {
    "tool": "@@MISE_REGISTRY_BIN_DISCOVERY_TOOL@@",
    "versions": "@@MISE_REGISTRY_BIN_DISCOVERY_VERSIONS@@",
    "bins": "@@MISE_REGISTRY_BIN_DISCOVERY_BINS@@",
}
CONTAINER_SCRIPT = r"""
set -eu
mkdir -p /state/home /state/config /state/data /state/cache /state/state /state/downloads /state/work
cd /state/work
mise test-tool --include-non-defined --jobs=1 "${DISCOVERY_TOOL}" 1>&2 || true
mise use --yes --pin "${DISCOVERY_TOOL}@latest" 1>&2
printf '%s\n' '@@MISE_REGISTRY_BIN_DISCOVERY_TOOL@@'
mise tool --json "${DISCOVERY_TOOL}"
printf '%s\n' '@@MISE_REGISTRY_BIN_DISCOVERY_VERSIONS@@'
mise ls --json --installed "${DISCOVERY_TOOL}"
printf '%s\n' '@@MISE_REGISTRY_BIN_DISCOVERY_BINS@@'
mise bin-paths --json "${DISCOVERY_TOOL}"
""".strip()


@dataclass(frozen=True)
class SandboxOptions:
    engine: str
    image: str
    platform: str
    memory: str
    cpus: str
    pids: int
    state_size: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--all", action="store_true", help="discover every registry tool")
    parser.add_argument("tools", nargs="*", help="registry tool shorthands to discover")
    parser.add_argument("--engine", choices=("auto", "docker", "podman"), default="auto")
    parser.add_argument("--image", help="prebuilt discovery image containing /usr/local/bin/mise")
    parser.add_argument("--mise-bin", type=Path, default=Path("target/debug/mise"))
    parser.add_argument("--platform", default="linux/amd64")
    parser.add_argument("--output", type=Path, default=Path("registry-bin-discovery.json"))
    parser.add_argument("--logs-dir", type=Path, default=Path("registry-bin-discovery-logs"))
    parser.add_argument("--jobs", type=positive_int, default=4)
    parser.add_argument("--timeout", type=positive_int, default=900)
    parser.add_argument("--memory", default="2g")
    parser.add_argument("--cpus", default="2")
    parser.add_argument("--pids", type=positive_int, default=512)
    parser.add_argument("--state-size", default="4g")
    parser.add_argument("--shard-count", type=positive_int, default=1)
    parser.add_argument("--shard-index", type=nonnegative_int, default=0)
    parser.add_argument("--limit", type=positive_int)
    parser.add_argument(
        "--skip-failures",
        action="store_true",
        help="do not retry failed tools already present in the output",
    )
    args = parser.parse_args()
    if args.all and args.tools:
        parser.error("tool arguments cannot be combined with --all")
    if not args.all and not args.tools:
        parser.error("provide one or more tools, or use --all")
    if args.shard_index >= args.shard_count:
        parser.error("--shard-index must be less than --shard-count")
    return args


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return parsed


def nonnegative_int(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be nonnegative")
    return parsed


def find_engine(requested: str) -> str:
    if requested != "auto":
        if not shutil.which(requested):
            raise RuntimeError(f"container engine not found: {requested}")
        return requested
    for engine in ("podman", "docker"):
        if shutil.which(engine):
            return engine
    raise RuntimeError("no supported container engine found (podman or docker)")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def prepare_image(engine: str, mise_bin: Path, platform: str, requested: str | None) -> str:
    if requested:
        return requested
    mise_bin = mise_bin.resolve()
    if not mise_bin.is_file():
        raise RuntimeError(f"mise binary not found: {mise_bin}")
    digest = sha256_file(mise_bin)[:16]
    platform_tag = re.sub(r"[^a-zA-Z0-9_.-]", "-", platform)
    image = (
        f"mise-registry-bin-discovery:v{IMAGE_RECIPE_VERSION}-{digest}-{platform_tag}"
    )
    inspected = subprocess.run(
        [engine, "image", "inspect", image],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if inspected.returncode == 0:
        return image

    with tempfile.TemporaryDirectory(prefix="mise-bin-discovery-image-") as raw_context:
        context = Path(raw_context)
        image_bin = context / "mise"
        objcopy = shutil.which("objcopy")
        if objcopy:
            stripped = subprocess.run(
                [objcopy, "--strip-debug", str(mise_bin), str(image_bin)],
                capture_output=True,
                text=True,
                check=False,
            )
            if stripped.returncode != 0:
                shutil.copy2(mise_bin, image_bin)
        else:
            shutil.copy2(mise_bin, image_bin)
        image_bin.chmod(0o755)
        (context / "Dockerfile").write_text(
            "FROM ubuntu:24.04\n"
            "RUN apt-get update && apt-get install -y --no-install-recommends "
            "bash build-essential ca-certificates curl git tar unzip xz-utils "
            "&& rm -rf /var/lib/apt/lists/*\n"
            "COPY mise /usr/local/bin/mise\n"
            "USER 65534:65534\n",
            encoding="utf-8",
        )
        subprocess.run(
            [engine, "build", "--platform", platform, "--tag", image, str(context)],
            check=True,
        )
    return image


def container_base_args(options: SandboxOptions) -> list[str]:
    return [
        options.engine,
        "run",
        "--rm",
        "--platform",
        options.platform,
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges:true",
        f"--pids-limit={options.pids}",
        f"--memory={options.memory}",
        f"--cpus={options.cpus}",
        "--tmpfs",
        f"/state:rw,exec,nosuid,nodev,mode=1777,size={options.state_size}",
        "--tmpfs",
        "/tmp:rw,exec,nosuid,nodev,mode=1777,size=256m",
        "--env",
        "HOME=/state/home",
        "--env",
        "MISE_CONFIG_DIR=/state/config",
        "--env",
        "MISE_DATA_DIR=/state/data",
        "--env",
        "MISE_CACHE_DIR=/state/cache",
        "--env",
        "MISE_STATE_DIR=/state/state",
        "--env",
        "MISE_DOWNLOADS_DIR=/state/downloads",
        options.image,
    ]


def list_registry_tools(options: SandboxOptions, timeout: int) -> list[str]:
    command = container_base_args(options) + [
        "/usr/local/bin/mise",
        "registry",
        "--json",
        "--hide-aliased",
    ]
    result = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"failed to list registry tools: {result.stderr.strip()}")
    tools = json.loads(result.stdout)
    return sorted(tool["short"] for tool in tools)


def parse_sections(stdout: str) -> dict[str, Any]:
    marker_names = {marker: name for name, marker in MARKERS.items()}
    sections: dict[str, list[str]] = {}
    current: str | None = None
    for line in stdout.splitlines():
        if line in marker_names:
            current = marker_names[line]
            sections[current] = []
        elif current is not None:
            sections[current].append(line)
    missing = sorted(set(MARKERS) - set(sections))
    if missing:
        raise ValueError(f"missing output sections: {', '.join(missing)}")
    return {name: json.loads("\n".join(lines)) for name, lines in sections.items()}


def normalize_bins(entries: list[dict[str, Any]]) -> list[str]:
    bins = set()
    for entry in entries:
        name = entry.get("name")
        if not isinstance(name, str):
            continue
        if name.lower().endswith(".exe"):
            name = name[:-4]
        if not name or "/" in name or "\\" in name:
            continue
        bins.add(name)
    return sorted(bins)


def installed_version(tool: str, versions: Any) -> str:
    records = versions.get(tool) if isinstance(versions, dict) else versions
    if not isinstance(records, list) or len(records) != 1:
        raise ValueError(f"expected one installed version for {tool}")
    version = records[0].get("version")
    if not isinstance(version, str) or not version:
        raise ValueError(f"missing concrete installed version for {tool}")
    return version


def safe_log_name(tool: str) -> str:
    return re.sub(r"[^a-zA-Z0-9_.-]", "_", tool) + ".log"


def classify_failure(returncode: int | None, stdout: str, timed_out: bool) -> str:
    if timed_out:
        return "timeout"
    if returncode is None:
        return "container"
    markers = [marker for marker in MARKERS.values() if marker in stdout]
    if not markers:
        return "install"
    if MARKERS["versions"] not in stdout:
        return "tool_metadata"
    if MARKERS["bins"] not in stdout:
        return "version_metadata"
    return "bin_metadata"


def discover_tool(
    tool: str,
    options: SandboxOptions,
    timeout: int,
    logs_dir: Path,
) -> dict[str, Any]:
    name_suffix = hashlib.sha256(
        f"{os.getpid()}:{threading.get_ident()}:{tool}:{time.time_ns()}".encode()
    ).hexdigest()[:12]
    container_name = f"mise-bin-discovery-{name_suffix}"
    command = container_base_args(options)
    command[-1:-1] = [
        "--name",
        container_name,
        "--env",
        f"DISCOVERY_TOOL={tool}",
    ]
    command.extend(["/bin/sh", "-c", CONTAINER_SCRIPT])
    started = time.monotonic()
    timed_out = False
    try:
        process = subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        returncode = process.returncode
        stdout = process.stdout
        stderr = process.stderr
    except subprocess.TimeoutExpired as error:
        timed_out = True
        returncode = None
        stdout = decode_timeout_output(error.stdout)
        stderr = decode_timeout_output(error.stderr)
        subprocess.run(
            [options.engine, "rm", "--force", container_name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )

    duration = round(time.monotonic() - started, 3)
    logs_dir.mkdir(parents=True, exist_ok=True)
    log_path = logs_dir / safe_log_name(tool)
    log_path.write_text(
        f"command: {json.dumps(command)}\n"
        f"returncode: {returncode}\n"
        f"stdout:\n{stdout}\n"
        f"stderr:\n{stderr}\n",
        encoding="utf-8",
    )

    base = {
        "tool": tool,
        "platform": options.platform,
        "duration_seconds": duration,
        "log": str(log_path),
    }
    if timed_out or returncode != 0:
        return {
            **base,
            "status": "failed",
            "error_category": classify_failure(returncode, stdout, timed_out),
            "error": last_error_line(stderr) or f"container exited with {returncode}",
        }
    try:
        sections = parse_sections(stdout)
        backend = sections["tool"].get("backend")
        if not isinstance(backend, str) or not backend:
            raise ValueError("missing resolved backend")
        version = installed_version(tool, sections["versions"])
        bins = normalize_bins(sections["bins"])
        if not bins:
            raise ValueError("no executable bins discovered")
        return {
            **base,
            "status": "success",
            "backend": backend,
            "version": version,
            "bins": bins,
        }
    except (json.JSONDecodeError, TypeError, ValueError) as error:
        return {
            **base,
            "status": "failed",
            "error_category": "output",
            "error": str(error),
        }


def decode_timeout_output(output: bytes | str | None) -> str:
    if output is None:
        return ""
    if isinstance(output, bytes):
        return output.decode(errors="replace")
    return output


def last_error_line(stderr: str) -> str | None:
    lines = [line.strip() for line in stderr.splitlines() if line.strip()]
    return lines[-1] if lines else None


def load_results(path: Path, platform: str) -> dict[str, dict[str, Any]]:
    if not path.exists():
        return {}
    artifact = json.loads(path.read_text(encoding="utf-8"))
    if artifact.get("schema_version") != SCHEMA_VERSION:
        raise RuntimeError(f"unsupported discovery schema in {path}")
    if artifact.get("platform") != platform:
        raise RuntimeError(
            f"output platform {artifact.get('platform')!r} does not match {platform!r}"
        )
    return {result["tool"]: result for result in artifact.get("results", [])}


def collisions(results: dict[str, dict[str, Any]]) -> dict[str, list[str]]:
    providers: dict[str, list[str]] = {}
    for tool, result in results.items():
        if result.get("status") != "success":
            continue
        for bin_name in result.get("bins", []):
            providers.setdefault(bin_name, []).append(tool)
    return {
        bin_name: sorted(tools)
        for bin_name, tools in sorted(providers.items())
        if len(tools) > 1
    }


def write_artifact(
    path: Path,
    platform: str,
    results: dict[str, dict[str, Any]],
) -> None:
    artifact = {
        "schema_version": SCHEMA_VERSION,
        "platform": platform,
        "results": [results[tool] for tool in sorted(results)],
        "collisions": collisions(results),
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", dir=path.parent, delete=False
    ) as file:
        json.dump(artifact, file, indent=2, sort_keys=True)
        file.write("\n")
        temp_path = Path(file.name)
    os.replace(temp_path, path)


def select_tools(
    requested: list[str],
    available: list[str],
    existing: dict[str, dict[str, Any]],
    shard_count: int,
    shard_index: int,
    limit: int | None,
    skip_failures: bool,
) -> list[str]:
    available_set = set(available)
    unknown = sorted(set(requested) - available_set)
    if unknown:
        raise RuntimeError(f"tools not found in target registry: {', '.join(unknown)}")
    selected = sorted(requested)
    selected = [tool for index, tool in enumerate(selected) if index % shard_count == shard_index]
    selected = [
        tool
        for tool in selected
        if tool not in existing
        or (existing[tool].get("status") != "success" and not skip_failures)
    ]
    return selected[:limit] if limit is not None else selected


def print_summary(results: dict[str, dict[str, Any]]) -> None:
    succeeded = sum(result.get("status") == "success" for result in results.values())
    failed = sum(result.get("status") == "failed" for result in results.values())
    print(
        f"results: {succeeded} succeeded, {failed} failed, "
        f"{len(collisions(results))} executable collisions"
    )


def main() -> int:
    args = parse_args()
    try:
        engine = find_engine(args.engine)
        image = prepare_image(engine, args.mise_bin, args.platform, args.image)
        options = SandboxOptions(
            engine=engine,
            image=image,
            platform=args.platform,
            memory=args.memory,
            cpus=args.cpus,
            pids=args.pids,
            state_size=args.state_size,
        )
        available = list_registry_tools(options, args.timeout)
        requested = available if args.all else args.tools
        results = load_results(args.output, args.platform)
        targets = select_tools(
            requested,
            available,
            results,
            args.shard_count,
            args.shard_index,
            args.limit,
            args.skip_failures,
        )
    except (OSError, RuntimeError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1

    if not targets:
        print("no tools to discover")
        print_summary(results)
        return 0

    print(f"discovering {len(targets)} tools with {args.jobs} workers using {engine}")
    lock = threading.Lock()
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as executor:
        futures = {
            executor.submit(discover_tool, tool, options, args.timeout, args.logs_dir): tool
            for tool in targets
        }
        for future in concurrent.futures.as_completed(futures):
            tool = futures[future]
            try:
                result = future.result()
            except Exception as error:  # Preserve progress if one worker has an unexpected bug.
                result = {
                    "tool": tool,
                    "platform": args.platform,
                    "status": "failed",
                    "error_category": "runner",
                    "error": str(error),
                }
            with lock:
                results[tool] = result
                write_artifact(args.output, args.platform, results)
            print(f"{tool}: {result['status']}")

    print_summary(results)
    return 0 if all(results[tool]["status"] == "success" for tool in targets) else 1


if __name__ == "__main__":
    raise SystemExit(main())
