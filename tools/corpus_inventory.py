#!/usr/bin/env python3
"""Extract, fetch, and audit the external model configuration corpus.

The tool intentionally uses only the Python standard library. It treats every
configuration file as inert bytes or JSON data; it never imports or executes
code referenced by a configuration document.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import defaultdict
from pathlib import Path, PurePosixPath
from typing import Any, Iterable, Sequence


SCHEMA_VERSION = 1
SUPPORTED_EXACT_NAMES = frozenset(
    {
        "adapter_config.json",
        "chat_template.jinja",
        "config.json",
        "generation_config.json",
        "model_index.json",
        "preprocessor_config.json",
        "processor_config.json",
        "quantization_config.json",
        "scheduler_config.json",
        "special_tokens_map.json",
        "tokenizer_config.json",
    }
)
JSON_KINDS = tuple(sorted(name for name in SUPPORTED_EXACT_NAMES if name.endswith(".json"))) + (
    "*.safetensors.index.json",
)
USER_AGENT = "dinoml-model-configs-rs-corpus/0.1"

HUGGING_FACE_URL_RE = re.compile(
    r"https?://(?:www\.)?huggingface\.co/([^\s/?#<>()\[\]]+)/([^\s/?#<>()\[\]]+)",
    re.IGNORECASE,
)
CONFIG_PATH_RE = re.compile(
    r"(?:[A-Za-z]:)?[/\\]configs[/\\]([A-Za-z0-9][A-Za-z0-9_.-]*)"
    r"[/\\]([A-Za-z0-9][A-Za-z0-9_.-]*)",
    re.IGNORECASE,
)
SOURCE_SNAPSHOT_RE = re.compile(
    r"_sources[/\\]([A-Za-z0-9][A-Za-z0-9_.-]*)__([A-Za-z0-9][A-Za-z0-9_.-]*)",
    re.IGNORECASE,
)
REPOSITORY_ID_RE = re.compile(
    r"(?<![A-Za-z0-9_.-])([A-Za-z0-9][A-Za-z0-9_.-]*)/"
    r"([A-Za-z0-9][A-Za-z0-9_.-]*)(?![A-Za-z0-9_.-])"
)
MODEL_ID_LABEL_RE = re.compile(
    r"\b(?:model|checkpoint)(?:\s+(?:repository|repo))?\s+id(?:\(s\)|s)?\s*:",
    re.IGNORECASE,
)
OTHER_FIELD_LABEL_RE = re.compile(r"^\s*[A-Za-z][A-Za-z0-9 /_()+.-]{1,48}:\s*")
CONTEXT_RE = re.compile(
    r"\b(?:checkpoint|hugging\s+face|hub\s+repo|model\s+repo|model\s+id|repository)\b",
    re.IGNORECASE,
)
URL_RESERVED_FIRST_SEGMENTS = frozenset({"api", "datasets", "docs", "organizations", "settings", "spaces"})
PATH_LIKE_OWNERS = frozenset(
    {
        "agents",
        "components",
        "config",
        "configs",
        "decoder",
        "docs",
        "encoder",
        "examples",
        "feature_extractor",
        "image_encoder",
        "model",
        "models",
        "pipeline",
        "pipelines",
        "plans",
        "processor",
        "processors",
        "references",
        "scheduler",
        "schedulers",
        "scripts",
        "snapshots",
        "src",
        "test",
        "tests",
        "text_encoder",
        "tokenizer",
        "tokenizers",
        "tools",
        "unet",
        "utils",
        "vae",
    }
)
PATH_LIKE_REPOSITORY_SUFFIXES = (
    ".bin",
    ".jinja",
    ".json",
    ".md",
    ".png",
    ".py",
    ".rs",
    ".safetensors",
    ".toml",
    ".txt",
    ".yaml",
    ".yml",
)

MAX_REPOSITORY_PATH_BYTES = 1024
MAX_REPOSITORY_PATH_SEGMENT_BYTES = 255
WINDOWS_RESERVED_NAMES = {"CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"}


def _portable_segment(segment: str) -> bool:
    try:
        if len(segment.encode("utf-8")) > MAX_REPOSITORY_PATH_SEGMENT_BYTES:
            return False
    except UnicodeEncodeError:
        return False
    if segment.endswith((".", " ")) or any(
        ord(character) <= 0x1F or character in '<>"|?*:' for character in segment
    ):
        return False
    stem = segment.split(".", 1)[0]
    if stem.endswith((".", " ")):
        return False
    upper = stem.upper()
    if upper in WINDOWS_RESERVED_NAMES:
        return False
    for prefix in ("COM", "LPT"):
        if upper.startswith(prefix) and upper.removeprefix(prefix) in {
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"
        }:
            return False
    return True


def _normalized_relative_path(path: str) -> PurePosixPath | None:
    if "\\" in path:
        return None
    try:
        if len(path.encode("utf-8")) > MAX_REPOSITORY_PATH_BYTES:
            return None
    except UnicodeEncodeError:
        return None
    raw_parts = path.split("/")
    if any(
        part in {"", ".", ".."}
        or part.startswith(".")
        or not _portable_segment(part)
        for part in raw_parts
    ):
        return None
    candidate = PurePosixPath(path)
    if candidate.is_absolute() or not candidate.parts:
        return None
    return candidate


def supported_kind(path: str) -> str | None:
    """Return the supported document kind for a safe repository-relative path."""
    candidate = _normalized_relative_path(path)
    if candidate is None:
        return None
    name = candidate.name
    if name in SUPPORTED_EXACT_NAMES:
        return name
    if name.endswith(".safetensors.index.json") and len(name) > len(".safetensors.index.json"):
        return "*.safetensors.index.json"
    return None


def _clean_repository_id(owner: str, repository: str, *, from_url: bool = False) -> str | None:
    owner = urllib.parse.unquote(owner).strip("`'\".,;:()[]{}")
    repository = urllib.parse.unquote(repository).strip("`'\".,;:()[]{}")
    if repository.endswith(".git"):
        repository = repository[:-4]
    if not owner or not repository or len(owner) > 96 or len(repository) > 96:
        return None
    if owner.casefold() in PATH_LIKE_OWNERS:
        return None
    if from_url and owner.casefold() in URL_RESERVED_FIRST_SEGMENTS:
        return None
    if owner.endswith(".co") or repository.casefold().endswith(PATH_LIKE_REPOSITORY_SUFFIXES):
        return None
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", owner):
        return None
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", repository):
        return None
    return f"{owner}/{repository}"


def _candidate_ids(text: str) -> list[str]:
    candidates: set[str] = set()
    text_without_urls = HUGGING_FACE_URL_RE.sub(" ", text)
    for match in REPOSITORY_ID_RE.finditer(text_without_urls):
        candidate = _clean_repository_id(match.group(1), match.group(2))
        if candidate is not None:
            candidates.add(candidate)
    return sorted(candidates, key=lambda value: (value.casefold(), value))


def extract_report_repositories(report_roots: Sequence[tuple[str, Path]]) -> list[dict[str, Any]]:
    """Extract repository IDs and line-level evidence from report.md trees."""
    evidence_by_id: dict[str, set[tuple[str, int, str]]] = defaultdict(set)

    for label, root in sorted(report_roots, key=lambda item: item[0]):
        for report_path in sorted(root.rglob("report.md"), key=lambda path: path.as_posix().casefold()):
            report_name = f"{label}/{report_path.relative_to(root).as_posix()}"
            text = report_path.read_text(encoding="utf-8-sig")
            in_model_id_block = False

            for line_number, line in enumerate(text.splitlines(), start=1):
                stripped = line.strip()
                if in_model_id_block and (not stripped or (OTHER_FIELD_LABEL_RE.match(line) and not line.lstrip().startswith("-"))):
                    in_model_id_block = False

                for match in HUGGING_FACE_URL_RE.finditer(line):
                    candidate = _clean_repository_id(match.group(1), match.group(2), from_url=True)
                    if candidate is not None:
                        evidence_by_id[candidate].add((report_name, line_number, "huggingface_url"))

                for match in CONFIG_PATH_RE.finditer(line):
                    candidate = _clean_repository_id(match.group(1), match.group(2))
                    if candidate is not None:
                        evidence_by_id[candidate].add((report_name, line_number, "config_path"))

                for match in SOURCE_SNAPSHOT_RE.finditer(line):
                    candidate = _clean_repository_id(match.group(1), match.group(2))
                    if candidate is not None:
                        evidence_by_id[candidate].add((report_name, line_number, "source_snapshot"))

                label_match = MODEL_ID_LABEL_RE.search(line)
                if label_match is not None:
                    in_model_id_block = True
                    for candidate in _candidate_ids(line[label_match.end() :]):
                        evidence_by_id[candidate].add((report_name, line_number, "model_id"))
                elif in_model_id_block:
                    for candidate in _candidate_ids(line):
                        evidence_by_id[candidate].add((report_name, line_number, "model_id_continuation"))
                elif CONTEXT_RE.search(line):
                    for candidate in _candidate_ids(line):
                        evidence_by_id[candidate].add((report_name, line_number, "contextual_id"))

    repositories: list[dict[str, Any]] = []
    for repository_id in sorted(evidence_by_id, key=lambda value: (value.casefold(), value)):
        evidence = [
            {"report": report, "line": line, "kind": kind}
            for report, line, kind in sorted(evidence_by_id[repository_id])
        ]
        repositories.append({"id": repository_id, "evidence": evidence})
    return repositories


def _strict_json_loads(data: bytes) -> tuple[Any, list[str]]:
    duplicate_keys: list[str] = []

    def reject_constant(value: str) -> None:
        raise ValueError(f"non-standard JSON constant {value}")

    def object_from_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                duplicate_keys.append(key)
            result[key] = value
        return result

    document = json.loads(
        data.decode("utf-8"),
        parse_constant=reject_constant,
        object_pairs_hook=object_from_pairs,
    )
    return document, duplicate_keys


def _json_type_name(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, dict):
        return "object"
    if isinstance(value, list):
        return "array"
    if isinstance(value, str):
        return "string"
    return "number"


def _json_error(error: Exception) -> dict[str, Any]:
    if isinstance(error, UnicodeDecodeError):
        return {"message": "input is not valid UTF-8", "byte": error.start}
    if isinstance(error, json.JSONDecodeError):
        return {"message": error.msg, "line": error.lineno, "column": error.colno}
    return {"message": str(error)}


def _load_fetch_revisions(fetch_manifest_path: Path | None) -> dict[str, dict[str, Any]]:
    if fetch_manifest_path is None or not fetch_manifest_path.is_file():
        return {}
    manifest = json.loads(fetch_manifest_path.read_text(encoding="utf-8"))
    return {
        entry["id"].casefold(): entry
        for entry in manifest.get("repositories", [])
        if isinstance(entry, dict) and isinstance(entry.get("id"), str)
    }


def audit_corpus(
    corpus_root: Path,
    report_repository_ids: Sequence[str] = (),
    fetch_manifest_path: Path | None = None,
    duplicate_sample_limit: int = 5,
) -> dict[str, Any]:
    """Audit supported documents below two-segment owner/repository roots."""
    repository_ids = sorted(set(report_repository_ids), key=lambda value: (value.casefold(), value))
    requested_by_casefold = {repository_id.casefold(): repository_id for repository_id in repository_ids}
    coverage: dict[str, dict[str, Any]] = {
        repository_id.casefold(): {
            "id": repository_id,
            "present": False,
            "supported_files": 0,
            "document_kinds": {},
        }
        for repository_id in repository_ids
    }
    revisions = _load_fetch_revisions(fetch_manifest_path)

    kind_counts: dict[str, int] = defaultdict(int)
    kind_hashes: dict[str, set[str]] = defaultdict(set)
    digest_paths: dict[tuple[str, str], list[str]] = defaultdict(list)
    invalid_json: list[dict[str, Any]] = []
    duplicate_json_keys: list[dict[str, Any]] = []
    json_top_level_types: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    empty_json_objects: dict[str, int] = defaultdict(int)
    repository_directories = 0
    repositories_with_supported_files = 0
    supported_files = 0
    supported_bytes = 0
    json_files = 0
    valid_json_files = 0

    owner_directories = sorted(
        (path for path in corpus_root.iterdir() if path.is_dir() and not path.name.startswith(".")),
        key=lambda path: (path.name.casefold(), path.name),
    )
    for owner_path in owner_directories:
        child_directories = sorted(
            (path for path in owner_path.iterdir() if path.is_dir() and not path.name.startswith(".")),
            key=lambda path: (path.name.casefold(), path.name),
        )
        for repository_path in child_directories:
            repository_directories += 1
            actual_id = f"{owner_path.name}/{repository_path.name}"
            actual_casefold = actual_id.casefold()
            requested_id = requested_by_casefold.get(actual_casefold)
            repository_kind_counts: dict[str, int] = defaultdict(int)
            repository_supported_files = 0

            for current_root, directory_names, file_names in os.walk(repository_path):
                directory_names[:] = sorted(name for name in directory_names if not name.startswith("."))
                for file_name in sorted(file_names):
                    path = Path(current_root) / file_name
                    relative_to_repository = path.relative_to(repository_path).as_posix()
                    kind = supported_kind(relative_to_repository)
                    if kind is None:
                        continue

                    relative_to_corpus = path.relative_to(corpus_root).as_posix()
                    data = path.read_bytes()
                    digest = hashlib.sha256(data).hexdigest()
                    supported_files += 1
                    repository_supported_files += 1
                    supported_bytes += len(data)
                    kind_counts[kind] += 1
                    repository_kind_counts[kind] += 1
                    kind_hashes[kind].add(digest)
                    digest_paths[(kind, digest)].append(relative_to_corpus)

                    if kind in JSON_KINDS:
                        json_files += 1
                        try:
                            document, duplicate_keys = _strict_json_loads(data)
                            valid_json_files += 1
                            json_top_level_types[kind][_json_type_name(document)] += 1
                            if document == {}:
                                empty_json_objects[kind] += 1
                            if duplicate_keys:
                                duplicate_json_keys.append(
                                    {
                                        "path": relative_to_corpus,
                                        "keys": sorted(set(duplicate_keys)),
                                        "occurrences": len(duplicate_keys),
                                    }
                                )
                        except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
                            invalid_json.append({"path": relative_to_corpus, **_json_error(error)})

            if repository_supported_files:
                repositories_with_supported_files += 1
            if requested_id is not None:
                entry = coverage[actual_casefold]
                entry["present"] = True
                entry["supported_files"] = repository_supported_files
                entry["document_kinds"] = dict(sorted(repository_kind_counts.items()))
                if actual_id != requested_id:
                    entry["filesystem_id"] = actual_id

    duplicate_groups: list[dict[str, Any]] = []
    duplicate_files = 0
    for (kind, digest), paths in digest_paths.items():
        if len(paths) < 2:
            continue
        sorted_paths = sorted(paths, key=lambda value: (value.casefold(), value))
        duplicate_files += len(sorted_paths) - 1
        duplicate_groups.append(
            {
                "kind": kind,
                "sha256": digest,
                "occurrences": len(sorted_paths),
                "sample_paths": sorted_paths[:duplicate_sample_limit],
                "paths_truncated": max(0, len(sorted_paths) - duplicate_sample_limit),
            }
        )
    duplicate_groups.sort(key=lambda entry: (-entry["occurrences"], entry["kind"], entry["sha256"]))

    coverage_entries: list[dict[str, Any]] = []
    for repository_id in repository_ids:
        entry = coverage[repository_id.casefold()]
        revision_entry = revisions.get(repository_id.casefold())
        if revision_entry is not None:
            if isinstance(revision_entry.get("revision"), str):
                entry["revision"] = revision_entry["revision"]
            if isinstance(revision_entry.get("status"), str):
                entry["fetch_status"] = revision_entry["status"]
        coverage_entries.append(entry)

    invalid_json.sort(key=lambda entry: (entry["path"].casefold(), entry["path"]))
    duplicate_json_keys.sort(key=lambda entry: (entry["path"].casefold(), entry["path"]))
    return {
        "schema_version": SCHEMA_VERSION,
        "corpus": {
            "repository_directories": repository_directories,
            "repositories_with_supported_files": repositories_with_supported_files,
            "supported_files": supported_files,
            "supported_bytes": supported_bytes,
            "json_files": json_files,
            "valid_json_files": valid_json_files,
            "invalid_json_files": len(invalid_json),
            "json_files_with_duplicate_keys": len(duplicate_json_keys),
        },
        "document_kinds": {
            kind: {"files": kind_counts[kind], "unique_contents": len(kind_hashes[kind])}
            for kind in sorted(kind_counts)
        },
        "duplicates": {
            "duplicate_files": duplicate_files,
            "groups": duplicate_groups,
        },
        "json_top_level_types": {
            kind: dict(sorted(type_counts.items()))
            for kind, type_counts in sorted(json_top_level_types.items())
        },
        "empty_json_objects": dict(sorted(empty_json_objects.items())),
        "invalid_json": invalid_json,
        "duplicate_json_keys": duplicate_json_keys,
        "report_repository_coverage": coverage_entries,
    }


def select_supported_files(siblings: Iterable[dict[str, Any]]) -> list[str]:
    """Select safe supported paths from a Hugging Face API siblings array."""
    selected: set[str] = set()
    for sibling in siblings:
        path = sibling.get("rfilename")
        if isinstance(path, str) and supported_kind(path) is not None:
            selected.add(_normalized_relative_path(path).as_posix())  # type: ignore[union-attr]
    return sorted(selected, key=lambda value: (value.casefold(), value))


def _http_bytes(url: str, token: str | None, attempts: int = 6) -> tuple[bytes, Any]:
    headers = {"User-Agent": USER_AGENT}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    for attempt in range(attempts):
        retry_delay = min(2**attempt, 30)
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.read(), response.headers
        except urllib.error.HTTPError as error:
            if error.code not in {429, 500, 502, 503, 504} or attempt + 1 == attempts:
                raise
            if error.code == 429:
                retry_after = error.headers.get("Retry-After")
                if retry_after is not None:
                    try:
                        retry_delay = min(max(float(retry_after), retry_delay), 30)
                    except ValueError:
                        pass
        except (TimeoutError, urllib.error.URLError):
            if attempt + 1 == attempts:
                raise
        time.sleep(retry_delay)
    raise RuntimeError("HTTP retry loop ended unexpectedly")


def _write_bytes_if_changed(path: Path, data: bytes) -> str:
    if path.is_file() and path.read_bytes() == data:
        return "unchanged"
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary_file:
        temporary_path = Path(temporary_file.name)
        temporary_file.write(data)
    try:
        os.replace(temporary_path, path)
    finally:
        if temporary_path.exists():
            temporary_path.unlink()
    return "updated" if path.exists() else "downloaded"


def _fetch_repository(repository_id: str, corpus_root: Path, token: str | None) -> dict[str, Any]:
    api_url = f"https://huggingface.co/api/models/{urllib.parse.quote(repository_id, safe='/')}"
    try:
        metadata_bytes, _ = _http_bytes(api_url, token)
        metadata = json.loads(metadata_bytes.decode("utf-8"))
        revision = metadata.get("sha")
        siblings = metadata.get("siblings", [])
        if not isinstance(revision, str) or not isinstance(siblings, list):
            return {"id": repository_id, "status": "invalid_api_response"}

        files: list[dict[str, Any]] = []
        for relative_path in select_supported_files(siblings):
            encoded_path = "/".join(urllib.parse.quote(part, safe="") for part in relative_path.split("/"))
            download_url = (
                f"https://huggingface.co/{urllib.parse.quote(repository_id, safe='/')}"
                f"/resolve/{revision}/{encoded_path}"
            )
            try:
                data, _ = _http_bytes(download_url, token)
                target = corpus_root.joinpath(*repository_id.split("/"), *relative_path.split("/"))
                existed = target.is_file()
                write_status = _write_bytes_if_changed(target, data)
                if not existed and write_status == "updated":
                    write_status = "downloaded"
                files.append(
                    {
                        "path": relative_path,
                        "status": write_status,
                        "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(),
                    }
                )
            except urllib.error.HTTPError as error:
                files.append({"path": relative_path, "status": f"http_{error.code}"})
            except (TimeoutError, urllib.error.URLError) as error:
                files.append({"path": relative_path, "status": "network_error", "error": str(error)})

        status = "ok" if all(file["status"] in {"downloaded", "unchanged", "updated"} for file in files) else "partial"
        return {"id": repository_id, "status": status, "revision": revision, "files": files}
    except urllib.error.HTTPError as error:
        status = {401: "unauthorized", 403: "forbidden", 404: "not_found"}.get(error.code, f"http_{error.code}")
        return {"id": repository_id, "status": status}
    except (TimeoutError, urllib.error.URLError) as error:
        return {"id": repository_id, "status": "network_error", "error": str(error)}
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        return {"id": repository_id, "status": "invalid_api_response", "error": str(error)}


def fetch_repositories(
    corpus_root: Path,
    repository_ids: Sequence[str],
    workers: int = 8,
    token: str | None = None,
) -> dict[str, Any]:
    """Fetch all supported files at each repository's resolved revision."""
    unique_ids = sorted(set(repository_ids), key=lambda value: (value.casefold(), value))
    results: list[dict[str, Any]] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        futures = {
            executor.submit(_fetch_repository, repository_id, corpus_root, token): repository_id
            for repository_id in unique_ids
        }
        for future in concurrent.futures.as_completed(futures):
            repository_id = futures[future]
            try:
                results.append(future.result())
            except Exception as error:  # keep one unexpected repository failure from losing the manifest
                results.append({"id": repository_id, "status": "error", "error": str(error)})
    results.sort(key=lambda entry: (entry["id"].casefold(), entry["id"]))
    return {
        "schema_version": SCHEMA_VERSION,
        "source": "huggingface.co",
        "selection": sorted(SUPPORTED_EXACT_NAMES) + ["*.safetensors.index.json"],
        "repositories": results,
    }


def merge_fetch_manifests(previous: dict[str, Any], retried: dict[str, Any]) -> dict[str, Any]:
    """Replace retried repository records while retaining prior successful records."""
    entries = {
        entry["id"]: entry
        for entry in previous.get("repositories", [])
        if isinstance(entry, dict) and isinstance(entry.get("id"), str)
    }
    for entry in retried.get("repositories", []):
        if isinstance(entry, dict) and isinstance(entry.get("id"), str):
            entries[entry["id"]] = entry
    return {
        "schema_version": retried.get("schema_version", previous.get("schema_version", SCHEMA_VERSION)),
        "source": retried.get("source", previous.get("source", "huggingface.co")),
        "selection": retried.get("selection", previous.get("selection", [])),
        "repositories": [entries[key] for key in sorted(entries, key=lambda value: (value.casefold(), value))],
    }


def resolve_report_candidates(
    candidates: dict[str, Any], fetch_manifest: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Split extracted candidates by whether Hub returned a concrete revision."""
    fetched_by_id = {
        entry["id"]: entry
        for entry in fetch_manifest.get("repositories", [])
        if isinstance(entry, dict) and isinstance(entry.get("id"), str)
    }
    resolved_entries: list[dict[str, Any]] = []
    unresolved_entries: list[dict[str, Any]] = []
    for candidate in candidates.get("repositories", []):
        if not isinstance(candidate, dict) or not isinstance(candidate.get("id"), str):
            continue
        repository_id = candidate["id"]
        fetched = fetched_by_id.get(repository_id, {})
        revision = fetched.get("revision")
        if isinstance(revision, str):
            documents: list[dict[str, Any]] = []
            for file_entry in fetched.get("files", []):
                if not isinstance(file_entry, dict) or not isinstance(file_entry.get("path"), str):
                    continue
                document = {"path": file_entry["path"]}
                if isinstance(file_entry.get("sha256"), str):
                    document["bytes"] = file_entry.get("bytes")
                    document["sha256"] = file_entry["sha256"]
                else:
                    document["status"] = file_entry.get("status", "unavailable")
                documents.append(document)
            documents.sort(key=lambda entry: (entry["path"].casefold(), entry["path"]))
            resolved_entries.append(
                {
                    "id": repository_id,
                    "revision": revision,
                    "fetch_status": fetched.get("status", "unknown"),
                    "evidence": candidate.get("evidence", []),
                    "documents": documents,
                }
            )
        else:
            unresolved_entries.append(
                {
                    "id": repository_id,
                    "fetch_status": fetched.get("status", "not_attempted"),
                    "evidence": candidate.get("evidence", []),
                }
            )
    resolved_entries.sort(key=lambda entry: (entry["id"].casefold(), entry["id"]))
    unresolved_entries.sort(key=lambda entry: (entry["id"].casefold(), entry["id"]))
    return (
        {
            "schema_version": SCHEMA_VERSION,
            "report_count": candidates.get("report_count", 0),
            "repository_count": len(resolved_entries),
            "repositories": resolved_entries,
        },
        {
            "schema_version": SCHEMA_VERSION,
            "report_count": candidates.get("report_count", 0),
            "candidate_count": len(unresolved_entries),
            "candidates": unresolved_entries,
        },
    )


def _retryable_fetch_status(status: Any) -> bool:
    if status in {"error", "http_429", "invalid_api_response", "network_error"}:
        return True
    return isinstance(status, str) and status.startswith("http_5")


def _retryable_fetch_entry(entry: dict[str, Any]) -> bool:
    if entry.get("status") != "partial":
        return _retryable_fetch_status(entry.get("status"))
    return any(
        isinstance(file_entry, dict) and _retryable_fetch_status(file_entry.get("status"))
        for file_entry in entry.get("files", [])
    )


def _atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary_file:
        temporary_path = Path(temporary_file.name)
        temporary_file.write(data)
    try:
        os.replace(temporary_path, path)
    finally:
        if temporary_path.exists():
            temporary_path.unlink()


def write_json(path: Path, document: Any) -> None:
    encoded = (json.dumps(document, indent=2, ensure_ascii=False, sort_keys=False) + "\n").encode("utf-8")
    _atomic_write(path, encoded)


def _load_repository_ids(path: Path) -> list[str]:
    document = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(document, list):
        entries = document
    elif isinstance(document, dict):
        entries = document.get("repositories", [])
    else:
        raise ValueError("repository manifest must be an array or object with a repositories array")
    repository_ids: list[str] = []
    for entry in entries:
        repository_id = entry if isinstance(entry, str) else entry.get("id") if isinstance(entry, dict) else None
        if not isinstance(repository_id, str) or repository_id.count("/") != 1:
            raise ValueError(f"invalid repository manifest entry: {entry!r}")
        owner, repository = repository_id.split("/", 1)
        if _clean_repository_id(owner, repository) != repository_id:
            raise ValueError(f"invalid repository manifest entry: {entry!r}")
        repository_ids.append(repository_id)
    return sorted(set(repository_ids), key=lambda value: (value.casefold(), value))


def render_markdown(audit: dict[str, Any]) -> str:
    corpus = audit["corpus"]
    coverage = audit["report_repository_coverage"]
    present = sum(1 for entry in coverage if entry["present"])
    with_documents = sum(1 for entry in coverage if entry["supported_files"] > 0)
    lines = [
        "# External corpus audit",
        "",
        "This is a deterministic summary. The third-party documents remain in the external corpus and are not committed.",
        "",
        "## Summary",
        "",
        f"- Repository directories: {corpus['repository_directories']:,}",
        f"- Repositories with supported documents: {corpus['repositories_with_supported_files']:,}",
        f"- Supported documents: {corpus['supported_files']:,}",
        f"- Valid JSON documents: {corpus['valid_json_files']:,} / {corpus['json_files']:,}",
        f"- Invalid JSON documents: {corpus['invalid_json_files']:,}",
        f"- JSON documents with duplicate object keys: {corpus['json_files_with_duplicate_keys']:,}",
        f"- Duplicate byte copies after the first occurrence: {audit['duplicates']['duplicate_files']:,}",
        f"- Report-referenced repositories present: {present:,} / {len(coverage):,}",
        f"- Report-referenced repositories with supported documents: {with_documents:,} / {len(coverage):,}",
        "",
        "## Document kinds",
        "",
        "| Kind | Files | Unique byte contents |",
        "|---|---:|---:|",
    ]
    for kind, counts in audit["document_kinds"].items():
        lines.append(f"| `{kind}` | {counts['files']:,} | {counts['unique_contents']:,} |")

    lines.extend(["", "## Semantic empty objects", ""])
    if audit["empty_json_objects"]:
        lines.extend(["| Kind | Empty objects |", "|---|---:|"])
        for kind, count in audit["empty_json_objects"].items():
            lines.append(f"| `{kind}` | {count:,} |")
    else:
        lines.append("No supported JSON document has an empty object as its root value.")

    lines.extend(["", "## Invalid JSON", ""])
    if audit["invalid_json"]:
        lines.extend(["| Path | Location | Error |", "|---|---:|---|"])
        for entry in audit["invalid_json"]:
            location = (
                f"{entry.get('line', '?')}:{entry.get('column', '?')}"
                if "line" in entry
                else f"byte {entry['byte']}" if "byte" in entry else "n/a"
            )
            message = str(entry["message"]).replace("|", "\\|")
            lines.append(f"| `{entry['path']}` | {location} | {message} |")
    else:
        lines.append("No invalid JSON was found among supported documents.")

    lines.extend(["", "## Duplicate object keys", ""])
    if audit["duplicate_json_keys"]:
        lines.extend(["| Path | Keys | Repeated occurrences |", "|---|---|---:|"])
        for entry in audit["duplicate_json_keys"]:
            keys = ", ".join(f"`{key}`" for key in entry["keys"])
            lines.append(f"| `{entry['path']}` | {keys} | {entry['occurrences']:,} |")
    else:
        lines.append("No duplicate JSON object keys were found.")

    missing = [entry for entry in coverage if not entry["present"] or entry["supported_files"] == 0]
    lines.extend(["", "## Missing report coverage", ""])
    if missing:
        lines.extend(["| Repository | Directory present | Fetch status |", "|---|---:|---|"])
        for entry in missing:
            lines.append(
                f"| `{entry['id']}` | {'yes' if entry['present'] else 'no'} | {entry.get('fetch_status', 'not recorded')} |"
            )
    else:
        lines.append("Every extracted report repository has at least one supported document.")
    lines.append("")
    return "\n".join(lines)


def _parse_report_root(value: str) -> tuple[str, Path]:
    if "=" not in value:
        raise argparse.ArgumentTypeError("report roots must use LABEL=PATH")
    label, raw_path = value.split("=", 1)
    path = Path(raw_path)
    if not label or not path.is_dir():
        raise argparse.ArgumentTypeError(f"report root does not exist: {value}")
    return label, path


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    extract_parser = subparsers.add_parser("extract", help="extract Hub repository IDs from report.md files")
    extract_parser.add_argument("--reports", action="append", required=True, type=_parse_report_root, metavar="LABEL=PATH")
    extract_parser.add_argument("--output", required=True, type=Path)

    fetch_parser = subparsers.add_parser("fetch", help="fetch supported documents at resolved Hub revisions")
    fetch_parser.add_argument("--corpus", required=True, type=Path)
    fetch_parser.add_argument("--repositories", required=True, type=Path)
    fetch_parser.add_argument("--metadata", required=True, type=Path)
    fetch_parser.add_argument("--workers", type=int, default=8)
    fetch_parser.add_argument(
        "--limit",
        type=int,
        help="process at most this many pending repositories (useful with anonymous Hub quotas)",
    )
    fetch_parser.add_argument(
        "--resume",
        action="store_true",
        help="retain completed metadata and retry only throttled, partial, or transient failures",
    )

    audit_parser = subparsers.add_parser("audit", help="audit an external corpus")
    audit_parser.add_argument("--corpus", required=True, type=Path)
    audit_parser.add_argument("--repositories", type=Path)
    audit_parser.add_argument("--fetch-manifest", type=Path)
    audit_parser.add_argument("--output", required=True, type=Path)
    audit_parser.add_argument("--markdown", type=Path)
    audit_parser.add_argument("--fail-invalid", action="store_true")

    resolve_parser = subparsers.add_parser(
        "resolve", help="separate revision-backed repositories from unresolved extraction candidates"
    )
    resolve_parser.add_argument("--candidates", required=True, type=Path)
    resolve_parser.add_argument("--fetch-manifest", required=True, type=Path)
    resolve_parser.add_argument("--output", required=True, type=Path)
    resolve_parser.add_argument("--unresolved-output", required=True, type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _build_parser()
    arguments = parser.parse_args(argv)

    if arguments.command == "extract":
        repositories = extract_report_repositories(arguments.reports)
        report_count = sum(1 for _, root in arguments.reports for _ in root.rglob("report.md"))
        manifest = {
            "schema_version": SCHEMA_VERSION,
            "report_count": report_count,
            "repository_count": len(repositories),
            "repositories": repositories,
        }
        write_json(arguments.output, manifest)
        print(f"extracted {len(repositories)} repositories from {report_count} reports")
        return 0

    if arguments.command == "fetch":
        if arguments.workers < 1 or arguments.workers > 32:
            parser.error("--workers must be between 1 and 32")
        if arguments.limit is not None and arguments.limit < 1:
            parser.error("--limit must be positive")
        repository_ids = _load_repository_ids(arguments.repositories)
        previous_manifest: dict[str, Any] | None = None
        if arguments.resume and arguments.metadata.is_file():
            previous_manifest = json.loads(arguments.metadata.read_text(encoding="utf-8"))
            previous_by_id = {
                entry["id"]: entry
                for entry in previous_manifest.get("repositories", [])
                if isinstance(entry, dict) and isinstance(entry.get("id"), str)
            }
            repository_ids = [
                repository_id
                for repository_id in repository_ids
                if repository_id not in previous_by_id
                or _retryable_fetch_entry(previous_by_id[repository_id])
            ]
        if arguments.limit is not None:
            repository_ids = repository_ids[: arguments.limit]
        fetched_manifest = fetch_repositories(
            arguments.corpus,
            repository_ids,
            workers=arguments.workers,
            token=os.environ.get("HF_TOKEN"),
        )
        manifest = (
            merge_fetch_manifests(previous_manifest, fetched_manifest)
            if previous_manifest is not None
            else fetched_manifest
        )
        write_json(arguments.metadata, manifest)
        statuses: dict[str, int] = defaultdict(int)
        for entry in manifest["repositories"]:
            statuses[entry["status"]] += 1
        print(json.dumps(dict(sorted(statuses.items())), separators=(",", ":")))
        return 0

    if arguments.command == "audit":
        repository_ids = _load_repository_ids(arguments.repositories) if arguments.repositories else []
        audit = audit_corpus(arguments.corpus, repository_ids, arguments.fetch_manifest)
        write_json(arguments.output, audit)
        if arguments.markdown:
            _atomic_write(arguments.markdown, render_markdown(audit).encode("utf-8"))
        print(json.dumps(audit["corpus"], separators=(",", ":")))
        return 1 if arguments.fail_invalid and audit["corpus"]["invalid_json_files"] else 0

    if arguments.command == "resolve":
        candidates = json.loads(arguments.candidates.read_text(encoding="utf-8"))
        fetch_manifest = json.loads(arguments.fetch_manifest.read_text(encoding="utf-8"))
        resolved, unresolved = resolve_report_candidates(candidates, fetch_manifest)
        write_json(arguments.output, resolved)
        write_json(arguments.unresolved_output, unresolved)
        print(
            json.dumps(
                {
                    "resolved": resolved["repository_count"],
                    "unresolved": unresolved["candidate_count"],
                },
                separators=(",", ":"),
            )
        )
        return 0

    parser.error(f"unsupported command: {arguments.command}")
    return 2


if __name__ == "__main__":
    sys.exit(main())
