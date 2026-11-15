#!/usr/bin/env python3
"""Project CLI entrypoint with interactive shell and persistent history/config."""

from __future__ import annotations

import argparse
import base64
import getpass
import importlib.metadata as importlib_metadata
import importlib.resources as importlib_resources
import json
import os
import shlex
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from collections.abc import Iterable
from pathlib import Path

import lexi  # the Rust engine (lexi-core) via its PyO3 wheel

# api_compose is retained only for CLI-side helpers the engine wheel doesn't
# expose: cli_flags_for, tool_preset_names, set_path/unset_path, _VALID_INCLUDES.
from lexi_cli import api_compose

try:
    from rich.console import Console
    from rich.table import Table
except Exception:  # pragma: no cover - optional dependency
    Console = None
    Table = None

try:
    import readline
except ImportError:  # pragma: no cover - readline not available on all platforms
    readline = None
try:
    import yaml  # type: ignore
except Exception:  # pragma: no cover - allow fallback
    yaml = None
try:
    from cryptography.fernet import Fernet
    from cryptography.hazmat.primitives import hashes
    from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC
except Exception:  # pragma: no cover - optional dependency
    Fernet = None

RED = "\033[31m"
RESET = "\033[0m"

CLI_ROOT = Path(__file__).resolve().parents[2]  # .../lexi-cli
REPO_ROOT = Path(__file__).resolve().parents[3]  # .../lexi
APP_DIR = Path.home() / ".lexi-cli"
LEGACY_APP_DIR = Path.home() / ".exp-cli"
CONFIG_FILE = APP_DIR / "config.json"
HISTORY_FILE = APP_DIR / "history"
PROMPTS_DIR = APP_DIR / "prompts"
PROVIDERS_ROOT = APP_DIR / "providers"
PROVIDERS_CONFIG = PROVIDERS_ROOT / "providers-config.yaml"
ALIASES_FILE = APP_DIR / "aliases.json"
DEFAULT_PROVIDERS_ROOT = REPO_ROOT / "providers"
if not DEFAULT_PROVIDERS_ROOT.exists():
    try:
        DEFAULT_PROVIDERS_ROOT = Path(importlib_resources.files("lexi_cli") / "providers")
    except Exception:
        DEFAULT_PROVIDERS_ROOT = REPO_ROOT / "providers"
DEFAULT_PROVIDERS_CONFIG = DEFAULT_PROVIDERS_ROOT / "providers-config.yaml"
APIS_DIR = APP_DIR / "apis"
DEFAULT_APIS_ROOT = REPO_ROOT / "providers" / "apis"
if not DEFAULT_APIS_ROOT.exists():
    try:
        DEFAULT_APIS_ROOT = Path(importlib_resources.files("lexi_cli") / "apis")
    except Exception:
        DEFAULT_APIS_ROOT = REPO_ROOT / "providers" / "apis"
try:
    DEFAULT_ALIASES_FILE = Path(importlib_resources.files("lexi_cli") / "default-aliases.json")
except Exception:
    DEFAULT_ALIASES_FILE = CLI_ROOT / "default-aliases.json"
LEGACY_CONFIG_FILE = CLI_ROOT / ".config.json"
LEGACY_HISTORY_FILE = CLI_ROOT / ".cli_history"

GENERAL_COMMANDS = {
    "help": "Show available commands and usage.",
    "sh": "Execute the rest of the line as a shell command.",
    "exit": "Exit the CLI.",
    "history": "Show or clear history. history [-n COUNT] [--reset|--clear]",
    "config": "View or update settings. config list [--raw|-r|key] | config set <key> <value> | config rm <key>",
    "alias": "Manage command aliases. alias list | alias add <name> <expansion> | alias rm <name> | alias reset",
    "version": "Show CLI version.",
}
AI_COMMANDS: dict[str, str] = {}

LEXI_COMMANDS = {
    "prompts": "Manage saved prompts or emit prompt JSON.",
    "providers": "Manage providers (list/add/rename/rm).",
    "models": "Manage models for a provider (list/add/set/rm).",
    "api": "Manage API definitions (list/show/create/edit/rm/validate).",
    "respond": "Send a prompt to a provider via the Responses API.",
}

COMMAND_SUBCOMMANDS = {
    "config": {
        "list": {
            "summary": "Show config values (optionally a single key or raw JSON).",
            "usage": "config list [--raw|-r|key]",
        },
        "set": {
            "summary": "Update a config entry.",
            "usage": "config set <key> <value>",
        },
        "rm": {
            "summary": "Remove a config entry.",
            "usage": "config rm <key>",
        },
    },
    "alias": {
        "list": {"summary": "List aliases.", "usage": "alias list"},
        "add": {"summary": "Create/update an alias.", "usage": "alias add <name> <expansion>"},
        "rm": {"summary": "Remove an alias.", "usage": "alias rm <name>"},
        "reset": {"summary": "Reset aliases to defaults.", "usage": "alias reset"},
    },
    "prompts": {
        "list": {
            "summary": "List prompts (optionally a single prompt).",
            "usage": "prompts list [name] [[-r|--raw] [-d|--detailed]]",
        },
        "set": {
            "summary": "Save or update a prompt definition.",
            "usage": "prompt set [name] [--prompt TEXT] [[-m|--max_tokens] N] [[-t|--temperature] T] [[-r|--role] ROLE] [--set path=value] [--unset path]",
        },
        "rm": {
            "summary": "Remove a saved prompt.",
            "usage": "prompt rm <name>",
        },
    },
    "providers": {
        "list": {"summary": "List configured providers.", "usage": "providers list [--raw|-r] [--table|-t]"},
        "add": {
            "summary": "Add a provider.",
            "usage": "providers add <name> --url URL --api_key KEY --default_model MODEL",
        },
        "api-key": {
            "summary": "Manage API keys stored in per-provider files.",
            "usage": "providers api-key set <name> [--value KEY] | providers api-key show <name> [--reveal] | providers api-key remove <name>",
        },
        "rename": {
            "summary": "Rename a provider.",
            "usage": "providers rename <old> <new>",
        },
        "rm": {"summary": "Remove a provider.", "usage": "providers rm <name>"},
        "status": {
            "summary": "Check provider status and API key validity.",
            "usage": "providers status <name> [--raw|-r]",
        },
    },
    "models": {
        "list": {
            "summary": "List models for a provider.",
            "usage": "models list <provider> [--raw|-r] [--table|-t] [--details|-d]",
        },
        "hosted": {
            "summary": "List models hosted by a provider via its API.",
            "usage": "models hosted <provider> [--raw|-r] [--table|-t]",
        },
        "default": {
            "summary": "View or set the default model for a provider.",
            "usage": "models default <provider> [model_name]",
        },
        "alias": {
            "summary": "Manage model aliases for a provider.",
            "usage": "models alias list <provider> [--raw|-r] | models alias set <provider> <alias> <model> | models alias rm <provider> <alias>",
        },
        "add": {
            "summary": "Add a model to a provider.",
            "usage": "model add <provider> <model> --name NAME [--description TEXT] [--max_tokens N] [--context_window N] [--default]",
        },
        "set": {
            "summary": "Update model metadata.",
            "usage": "model set <provider> <model> [--name NAME] [--description TEXT] [--max_tokens N] [--context_window N] [--default]",
        },
        "rm": {"summary": "Remove a model.", "usage": "model rm <provider> <model>"},
    },
    "api": {
        "list": {"summary": "List API definitions.", "usage": "api list [--raw|-r]"},
        "show": {"summary": "Show an API definition.", "usage": "api show <name> [--raw|-r]"},
        "create": {
            "summary": "Create a new API definition.",
            "usage": "api create <name> [--from <existing>] [--path PATH] [--text-path PATH]",
        },
        "edit": {
            "summary": "Edit an API definition.",
            "usage": "api edit <name> [--path PATH] [--text-path PATH] [--set-meta KEY:PATH]",
        },
        "rm": {"summary": "Remove an API definition.", "usage": "api rm <name>"},
        "validate": {"summary": "Validate an API definition.", "usage": "api validate <name>"},
    },
}

SHORTCUTS = {
    "?": "Shortcut for 'help'.",
    "!": "Shortcut for 'sh' in interactive mode.",
    "#<N>": "Re-run a command by its history number (#<N>, negatives are relative).",
    "↑ / ↓": "Navigate command history (previous/next).",
}
EXIT_COMMANDS = {"exit", "quit", "e", "q"}
CONFIG_DEFAULT = {
    "cli_name": "lexi",
    "prompt": "cmd",
    "prompt_delimiter": ">",
    "edit_mode": "vi",
}


def get_cli_version() -> str:
    """Read CLI version from installed package metadata (pyproject.toml)."""
    try:
        return importlib_metadata.version("lexi-cli")
    except importlib_metadata.PackageNotFoundError:
        return "dev"


CONFIG: dict[str, str] = CONFIG_DEFAULT.copy()
HISTORY: list[str] = []
PROMPT_DEFAULT = {
    "prompt": "",
    "max_tokens": 256,
    "temperature": 0.7,
    "role": "user",
}
PROMPTS: dict[str, dict] = {}
PROMPT_DEFAULTS_LOADED = False
PROMPT_RESOLVED_DEFAULT = PROMPT_DEFAULT.copy()
PROMPT_ACTIVE_NAME = "$$"
ALIASES: dict[str, str] = {}
console = Console() if Console else None


def load_yaml_file(path: Path) -> dict:
    if not path.exists():
        return {}
    try:
        if yaml:
            data = yaml.safe_load(path.read_text()) or {}
            return data if isinstance(data, dict) else {}
        # Fallback: basic JSON parse attempt
        return json.loads(path.read_text())
    except Exception as exc:
        print_error(f"Warning: could not read {path.name} ({exc}).")
        return {}


def save_yaml_file(path: Path, data: dict) -> bool:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        if yaml:
            path.write_text(yaml.safe_dump(data, sort_keys=False))
        else:
            path.write_text(json.dumps(data, indent=2))
        return True
    except Exception as exc:
        print_error(f"Warning: could not write {path.name} ({exc}).")
        return False


def load_api_definition(name: str) -> dict | None:
    """Load an API definition by name from the runtime apis directory."""
    path = APIS_DIR / f"{name}.yaml"
    if not path.exists():
        return None
    data = load_yaml_file(path)
    return data if data else None


def save_api_definition(name: str, data: dict) -> bool:
    """Save an API definition to the runtime apis directory."""
    path = APIS_DIR / f"{name}.yaml"
    return save_yaml_file(path, data)


def load_all_api_definitions() -> dict[str, dict]:
    """Load all API definitions from the runtime apis directory."""
    apis: dict[str, dict] = {}
    if not APIS_DIR.exists():
        return apis
    for path in sorted(APIS_DIR.glob("*.yaml")):
        data = load_yaml_file(path)
        if data:
            apis[path.stem] = data
    return apis


def delete_api_definition(name: str) -> bool:
    """Delete an API definition from the runtime apis directory."""
    path = APIS_DIR / f"{name}.yaml"
    if not path.exists():
        return False
    try:
        path.unlink()
        return True
    except OSError as exc:
        print_error(f"Could not delete API definition {name!r}: {exc}")
        return False


def validate_api_definition(data: dict) -> list[str]:
    """Validate a schema-driven API definition. Returns error messages (empty = valid)."""
    errors: list[str] = []
    if not isinstance(data.get("name"), str) or not data["name"]:
        errors.append("Missing or empty 'name' field.")
    req = data.get("request")
    if not isinstance(req, dict):
        errors.append("Missing 'request' block.")
    else:
        if not isinstance(req.get("path"), str) or not req["path"]:
            errors.append("Missing or empty 'request.path'.")
        body_schema = req.get("body_schema")
        if not isinstance(body_schema, dict):
            errors.append("Missing or invalid 'request.body_schema' (must be a dict).")
        else:
            if body_schema.get("type") not in (None, "object"):
                errors.append("'request.body_schema.type' must be 'object'.")
            props = body_schema.get("properties")
            if props is not None and not isinstance(props, dict):
                errors.append("'request.body_schema.properties' must be a dict.")
            elif isinstance(props, dict):
                for pname, pdef in props.items():
                    if not isinstance(pdef, dict):
                        errors.append(f"Property {pname!r} must be a dict.")
                        continue
                    inc = pdef.get("x-include")
                    if inc is not None and inc not in api_compose._VALID_INCLUDES:
                        errors.append(
                            f"Property {pname!r}: invalid x-include {inc!r} "
                            f"(expected one of {sorted(api_compose._VALID_INCLUDES)})"
                        )
        # Reject legacy keys outright (no backward compat).
        for legacy in ("body", "parameters", "param_mapping", "tools", "tools_key"):
            if legacy in req:
                errors.append(
                    f"'request.{legacy}' is from the legacy schema; "
                    f"use 'request.body_schema' instead."
                )
    presets = data.get("tool_presets")
    if presets is not None and not isinstance(presets, dict):
        errors.append("'tool_presets' must be a dict mapping name -> body fragment.")
    resp = data.get("response")
    if not isinstance(resp, dict):
        errors.append("Missing 'response' block.")
    else:
        # Reject legacy top-level text_path/text_fallbacks (they live inside
        # response.text_extract now).
        for legacy in ("text_path", "text_fallbacks"):
            if legacy in resp:
                errors.append(
                    f"'response.{legacy}' is from the legacy schema; "
                    f"use 'response.text_extract' instead."
                )
        te = resp.get("text_extract")
        arts = resp.get("artifacts")
        if te is None and arts is None:
            errors.append(
                "response must define 'text_extract' (text output) or "
                "'artifacts' (binary output)."
            )
        if te is not None and not isinstance(te, dict):
            errors.append("'response.text_extract' must be a dict.")
        elif isinstance(te, dict):
            has_path = isinstance(te.get("text_path"), str) and te["text_path"]
            has_items = isinstance(te.get("items_path"), str) and te["items_path"]
            if has_path and has_items:
                errors.append(
                    "'response.text_extract' must specify either text_path "
                    "(simple) OR items_path (walked), not both."
                )
            elif not has_path and not has_items:
                errors.append(
                    "'response.text_extract' must specify either text_path "
                    "or items_path."
                )
            for opt_dict in ("item_filter", "content_filter"):
                v = te.get(opt_dict)
                if v is not None and not isinstance(v, dict):
                    errors.append(
                        f"'response.text_extract.{opt_dict}' must be a dict of field=value pairs."
                    )
        if arts is not None and not isinstance(arts, dict):
            errors.append("'response.artifacts' must be a dict.")
        meta = resp.get("metadata")
        if meta is not None and not isinstance(meta, dict):
            errors.append("'response.metadata' must be a dict.")
        stream = resp.get("stream")
        if stream is not None:
            if not isinstance(stream, dict):
                errors.append("'response.stream' must be a dict.")
            else:
                ae = stream.get("activity_events")
                if ae is not None and not isinstance(ae, dict):
                    errors.append("'response.stream.activity_events' must be a dict.")
                elif isinstance(ae, dict):
                    for evname, msg in ae.items():
                        if not isinstance(msg, str):
                            errors.append(
                                f"'response.stream.activity_events.{evname}' must be a string."
                            )
        act = resp.get("activity")
        if act is not None:
            if not isinstance(act, dict):
                errors.append("'response.activity' must be a dict.")
            else:
                if "items_path" in act and not isinstance(act["items_path"], str):
                    errors.append("'response.activity.items_path' must be a string.")
                sbt = act.get("summarize_by_type")
                if sbt is not None and not isinstance(sbt, dict):
                    errors.append("'response.activity.summarize_by_type' must be a dict.")
                elif isinstance(sbt, dict):
                    for tname, label in sbt.items():
                        if not isinstance(label, str):
                            errors.append(
                                f"'response.activity.summarize_by_type.{tname}' must be a string."
                            )
        ann = resp.get("annotations")
        if ann is not None:
            if not isinstance(ann, dict):
                errors.append("'response.annotations' must be a dict.")
            else:
                scope = ann.get("scope", "text_items")
                if scope not in ("text_items", "response_root"):
                    errors.append(
                        f"'response.annotations.scope' must be 'text_items' or 'response_root' (got {scope!r})."
                    )
                types = ann.get("types")
                if types is not None and not isinstance(types, dict):
                    errors.append("'response.annotations.types' must be a dict mapping type → format spec.")
                elif isinstance(types, dict):
                    for tname, tcfg in types.items():
                        if not isinstance(tcfg, dict):
                            errors.append(
                                f"'response.annotations.types.{tname}' must be a dict (e.g. {{format: ...}})."
                            )
                            continue
                        if "format" in tcfg and not isinstance(tcfg["format"], str):
                            errors.append(
                                f"'response.annotations.types.{tname}.format' must be a string."
                            )

    op = data.get("operation")
    if op is not None:
        if not isinstance(op, dict):
            errors.append("'operation' must be a dict.")
        else:
            poll = op.get("poll")
            if poll is not None and not isinstance(poll, dict):
                errors.append("'operation.poll' must be a dict.")
            elif isinstance(poll, dict) and not isinstance(poll.get("path_template", ""), str):
                errors.append("'operation.poll.path_template' must be a string.")
            sm = op.get("status_map")
            if sm is not None and not isinstance(sm, dict):
                errors.append("'operation.status_map' must be a dict.")

    files = data.get("files")
    if files is not None:
        if not isinstance(files, dict):
            errors.append("'files' must be a dict.")
        else:
            up = files.get("upload")
            if up is not None and not isinstance(up, dict):
                errors.append("'files.upload' must be a dict.")
            elif isinstance(up, dict) and not isinstance(up.get("path", ""), str):
                errors.append("'files.upload.path' must be a string.")
    return errors


def seed_default_apis() -> None:
    """Copy bundled API definitions to ~/.lexi-cli/apis/ if missing."""
    if not DEFAULT_APIS_ROOT.exists():
        return
    try:
        APIS_DIR.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        print_error(f"Warning: could not prepare apis dir ({exc}).")
        return
    for src in DEFAULT_APIS_ROOT.glob("*.yaml"):
        dest = APIS_DIR / src.name
        if not dest.exists():
            try:
                shutil.copyfile(src, dest)
            except OSError as exc:
                print_error(f"Warning: could not seed API definition {src.stem} ({exc}).")


def expand_env_vars(value: object) -> object:
    """Recursively expand ${VAR} strings using the current environment."""
    if isinstance(value, dict):
        return {k: expand_env_vars(v) for k, v in value.items()}
    if isinstance(value, list):
        return [expand_env_vars(v) for v in value]
    if isinstance(value, str):
        return os.path.expandvars(value)
    return value


def load_aliases() -> None:
    global ALIASES
    if not ALIASES_FILE.exists():
        ALIASES = {}
        return
    try:
        data = json.loads(ALIASES_FILE.read_text())
        if isinstance(data, dict) and all(isinstance(v, list) for v in data.values()):
            # grouped format: command -> [aliases]
            expanded: dict[str, str] = {}
            for cmd, aliases in data.items():
                for alias in aliases:
                    if isinstance(alias, str):
                        expanded[alias] = cmd
            ALIASES = expanded
        elif isinstance(data, dict):
            ALIASES = {k: str(v) for k, v in data.items()}
        else:
            ALIASES = {}
    except Exception as exc:
        print_error(f"Warning: could not read aliases ({exc}); starting empty.")
        ALIASES = {}


def save_aliases() -> None:
    try:
        ALIASES_FILE.parent.mkdir(parents=True, exist_ok=True)
        grouped: dict[str, list[str]] = {}
        for alias, cmd in ALIASES.items():
            grouped.setdefault(cmd, []).append(alias)
        grouped = {k: sorted(v) for k, v in grouped.items()}
        ALIASES_FILE.write_text(json.dumps(grouped, indent=2))
    except Exception as exc:
        print_error(f"Warning: could not save aliases ({exc}).")


def reset_aliases_to_defaults() -> None:
    try:
        data = json.loads(DEFAULT_ALIASES_FILE.read_text())
        if isinstance(data, dict):
            ALIASES_FILE.parent.mkdir(parents=True, exist_ok=True)
            ALIASES_FILE.write_text(json.dumps(data, indent=2))
            load_aliases()
    except Exception as exc:
        print_error(f"Failed to reset aliases: {exc}")


def derive_key(passphrase: str, salt: bytes) -> bytes:
    if Fernet is None:
        raise RuntimeError("cryptography is required for encryption (pip install 'lexi-cli[crypto]').")
    kdf = PBKDF2HMAC(
        algorithm=hashes.SHA256(),
        length=32,
        salt=salt,
        iterations=390_000,
    )
    return base64.urlsafe_b64encode(kdf.derive(passphrase.encode()))


def encrypt_api_key(value: str, passphrase: str) -> dict:
    salt = os.urandom(16)
    key = derive_key(passphrase, salt)
    fernet = Fernet(key)
    token = fernet.encrypt(value.encode())
    return {"salt": base64.urlsafe_b64encode(salt).decode(), "token": token.decode()}


def decrypt_api_key(encrypted: dict, passphrase: str) -> str:
    salt_b64 = encrypted.get("salt")
    token = encrypted.get("token")
    if not salt_b64 or not token:
        raise ValueError("Encrypted key missing salt or token.")
    salt = base64.urlsafe_b64decode(salt_b64.encode())
    key = derive_key(passphrase, salt)
    fernet = Fernet(key)
    return fernet.decrypt(token.encode()).decode()


def extract_prompt_payload(name: str) -> dict | None:
    target = name or PROMPT_ACTIVE_NAME
    if target == "$$":
        target = PROMPT_ACTIVE_NAME
    payload = PROMPTS.get(target)
    if not payload:
        return None
    return payload


def resolve_path(data: object, path_expr: str) -> object | None:
    """Walk a dot/bracket path expression into nested dicts/lists.

    Examples: ``"content[0].text"``, ``"choices[0].message.content"``.
    Returns ``None`` when any segment fails to resolve.
    """
    import re

    tokens = re.split(r"\.(?![^\[]*\])", path_expr)
    current: object = data
    for token in tokens:
        parts = re.split(r"[\[\]]", token)
        parts = [p for p in parts if p != ""]
        for part in parts:
            if current is None:
                return None
            if part.isdigit():
                idx = int(part)
                if isinstance(current, list) and idx < len(current):
                    current = current[idx]
                else:
                    return None
            elif isinstance(current, dict):
                current = current.get(part)
            else:
                return None
    return current


def build_auth_headers(provider_cfg: dict, api_key: str) -> dict[str, str]:
    """Build HTTP headers from provider auth config with Bearer token defaults."""
    auth = provider_cfg.get("auth", {})
    header_name = auth.get("header", "Authorization")
    prefix = auth.get("value_prefix", "Bearer ")
    headers: dict[str, str] = {"Content-Type": "application/json", "Accept": "application/json"}
    headers[header_name] = f"{prefix}{api_key}"
    for k, v in auth.get("extra_headers", {}).items():
        headers[k] = str(v)
    return headers


# Body composition and response extraction are delegated to the `lexi` wheel
# (Rust lexi-core via PyO3); see the engine calls in respond_with_provider.


def deep_merge(base: dict, override: dict) -> dict:
    """Recursively merge *override* into *base*. Lists are replaced, not concatenated."""
    merged = dict(base)
    for key, val in override.items():
        if key in merged and isinstance(merged[key], dict) and isinstance(val, dict):
            merged[key] = deep_merge(merged[key], val)
        else:
            merged[key] = val
    return merged


def resolve_effective_config(provider: str, provider_cfg: dict) -> dict:
    """Build effective provider config from an API definition + inline overrides.

    Resolution:
      1. Provider must reference an API via ``api: <name>`` (or define
         ``request.body_schema`` inline).
      2. Inline ``auth``/``request``/``response``/``tool_presets`` from the
         provider config deep-merge over the API definition.
    """
    api_name = provider_cfg.get("api")
    if api_name:
        api_def = load_api_definition(api_name)
        if api_def:
            cfg = dict(provider_cfg)
            for key in ("auth", "request", "response", "tool_presets"):
                api_block = api_def.get(key)
                if api_block is None:
                    continue
                inline_block = cfg.get(key)
                if isinstance(inline_block, dict) and isinstance(api_block, dict):
                    cfg[key] = deep_merge(api_block, inline_block)
                elif key not in cfg:
                    cfg[key] = api_block
            return cfg
    # No api: reference — accept inline definition if present.
    return provider_cfg


def extract_response_metadata(data: dict, api_cfg: dict) -> dict[str, object]:
    """Extract metadata fields (model, tokens, stop reason) from response JSON."""
    resp = api_cfg.get("response", {})
    meta_paths = resp.get("metadata", {})
    if not isinstance(meta_paths, dict):
        return {}
    result: dict[str, object] = {}
    for key, path_expr in meta_paths.items():
        if isinstance(path_expr, str):
            val = resolve_path(data, path_expr)
            if val is not None:
                result[key] = val
    return result


def print_response_metadata(
    metadata: dict[str, object], activity_summary: str = ""
) -> None:
    """Print response metadata to stderr in dim styling.

    ``activity_summary`` (e.g., ``"1 web search · reasoning"``) is appended as
    a separate dim line when non-empty.
    """
    parts: list[str] = []
    model = metadata.get("model") if metadata else None
    if model:
        parts.append(str(model))
    in_tok = metadata.get("input_tokens") if metadata else None
    out_tok = metadata.get("output_tokens") if metadata else None
    total_tok = metadata.get("total_tokens") if metadata else None
    if in_tok is not None and out_tok is not None:
        tok_str = f"{in_tok} in + {out_tok} out"
        if total_tok is not None:
            tok_str += f" = {total_tok} tokens"
        parts.append(tok_str)
    elif total_tok is not None:
        parts.append(f"{total_tok} tokens")
    stop = metadata.get("stop_reason") if metadata else None
    if stop:
        parts.append(str(stop))
    lines: list[str] = []
    if parts:
        lines.append("[" + " | ".join(parts) + "]")
    if activity_summary:
        lines.append("[" + activity_summary + "]")
    if not lines:
        return
    if Console:
        stderr_console = Console(stderr=True)
        for line in lines:
            stderr_console.print(line, style="dim")
    else:
        for line in lines:
            print(f"\033[2m{line}\033[0m", file=sys.stderr)


def _read_sse_events(resp) -> Iterable[tuple[str | None, str]]:
    """Yield ``(event_name, data_str)`` pairs from a Server-Sent Events response.

    ``event_name`` is ``None`` for unnamed events (which is what the legacy
    Chat Completions stream uses). ``data_str`` is the concatenated payload of
    all ``data:`` lines for that event.
    """
    event_name: str | None = None
    data_lines: list[str] = []
    for raw_line in resp:
        try:
            line = raw_line.decode("utf-8", errors="replace")
        except Exception:
            continue
        # Strip the line terminator (\r\n or \n) but preserve internal spaces.
        line = line.rstrip("\r\n")
        if line == "":
            if data_lines:
                yield event_name, "\n".join(data_lines)
            event_name = None
            data_lines = []
            continue
        if line.startswith(":"):  # SSE comment / keep-alive
            continue
        if line.startswith("event:"):
            event_name = line[len("event:"):].strip()
        elif line.startswith("data:"):
            data_lines.append(line[len("data:"):].lstrip(" "))
        # id:, retry:, and unknown fields are ignored
    # Flush any trailing event without a terminating blank line.
    if data_lines:
        yield event_name, "\n".join(data_lines)


def _stream_sse_response(
    resp, stream_cfg: dict, *, suppress_text: bool = False
) -> dict | None:
    """Consume an SSE response, printing incremental text deltas as they arrive.

    ``stream_cfg`` declares how to extract deltas and the final response object.
    Optional ``stream_cfg.activity_events`` is a map of SSE event name → status
    message; matched events are printed to stderr (so they don't pollute stdout
    text) as live indicators while the request runs.

    Returns the final response dict if one was captured (for metadata / --raw).
    """
    delta_event = stream_cfg.get("delta_event")  # None = match unnamed/any
    delta_path = stream_cfg.get("delta_path") or ""
    done_event = stream_cfg.get("done_event")
    done_marker = stream_cfg.get("done_marker")
    final_event = stream_cfg.get("final_event") or done_event
    activity_events = stream_cfg.get("activity_events") or {}

    final_response: dict | None = None
    text_emitted = False

    for event_name, data_str in _read_sse_events(resp):
        if done_marker is not None and data_str == done_marker:
            break
        try:
            data = json.loads(data_str)
        except json.JSONDecodeError:
            continue
        # Activity indicator (stderr) — emitted before delta text so the user
        # sees "🔍 searching" first, then the eventual answer text on stdout.
        if event_name and event_name in activity_events:
            msg = activity_events[event_name]
            if isinstance(msg, str) and msg:
                _emit_activity_indicator(msg)
        matches_delta = (
            (delta_event is None and event_name in (None, ""))
            or (delta_event is not None and event_name == delta_event)
        )
        if matches_delta and delta_path:
            chunk = resolve_path(data, delta_path)
            if isinstance(chunk, str) and chunk and not suppress_text:
                sys.stdout.write(chunk)
                sys.stdout.flush()
                text_emitted = True
        if final_event and event_name == final_event and isinstance(data, dict):
            final_response = data
        if done_event and event_name == done_event:
            break

    if text_emitted:
        sys.stdout.write("\n")
        sys.stdout.flush()
    return final_response


def _emit_activity_indicator(msg: str) -> None:
    """Print a streaming activity indicator to stderr in dim styling."""
    if Console:
        stderr_console = Console(stderr=True)
        stderr_console.print(msg, style="dim")
    else:
        print(f"\033[2m{msg}\033[0m", file=sys.stderr, flush=True)


def respond_with_provider(
    provider: str,
    provider_cfg: dict,
    prompt_payload: dict,
    *,
    model_override: str | None = None,
    overrides: dict | None = None,
    set_paths: list[tuple[str, object]] | None = None,
    unset_paths: list[str] | None = None,
    tool_presets_selected: list[str] | None = None,
    raw_mode: bool = False,
) -> tuple[int, dict | None, bool]:
    """Send a prompt to ``provider``'s configured API.

    Returns ``(exit_code, response_json, was_streamed)``. When ``was_streamed``
    is True, incremental text has already been printed to stdout (unless
    ``raw_mode`` is set, in which case streaming text is suppressed and the
    caller is expected to render the final ``response_json``).

    Precedence (low → high): schema default → prompt_payload → overrides →
    tool presets → set_paths → unset_paths.
    """
    provider_cfg = resolve_effective_config(provider, provider_cfg)
    api_key = load_api_key(provider, provider_cfg)
    base_url = (provider_cfg.get("url") or "").rstrip("/")
    model = model_override or provider_cfg.get("default_model")
    if not api_key or not base_url or not model:
        print_error("Provider config missing url/api_key/default_model.")
        return 1, None, False

    req_cfg = provider_cfg.get("request", {})
    req_path = req_cfg.get("path")
    if not req_path:
        print_error(
            f"Provider {provider!r}: API definition has no request.path. "
            f"Check the 'api:' reference in providers-config.yaml."
        )
        return 1, None, False
    endpoint = f"{base_url}{req_path}"

    prompt_text = prompt_payload.get("prompt", "")
    if not prompt_text:
        print_error("Prompt text is empty.")
        return 1, None, False

    # Route the message through the canonical IR, lowering its content to the
    # provider's wire shape via the API def's content_mapping. For text chat this
    # is an identity transform (same body as before); image parts are additive.
    message = lexi.normalize_prompt_payload(prompt_payload)
    content_mapping = provider_cfg.get("content_mapping")
    effective_payload = {**prompt_payload, **lexi.lower_message(message, content_mapping)}

    merged_overrides: dict[str, object] = dict(overrides or {})
    merged_overrides.setdefault("model", model)

    try:
        body = lexi.compose_request_body(
            provider_cfg,
            prompt_payload=effective_payload,
            overrides=merged_overrides,
            tool_presets=tool_presets_selected,
            set_paths=set_paths,
            unset_paths=unset_paths,
        )
    except (ValueError, KeyError) as exc:
        print_error(str(exc))
        return 1, None, False

    is_stream = bool(body.get("stream"))
    headers = build_auth_headers(provider_cfg, api_key)
    if is_stream:
        headers.setdefault("Accept", "text/event-stream")
        headers["Accept"] = "text/event-stream"

    try:
        req = urllib.request.Request(endpoint, data=json.dumps(body).encode(), headers=headers)  # type: ignore[arg-type]
        with urllib.request.urlopen(req, timeout=60 if is_stream else 30) as resp:  # type: ignore[call-arg]
            if is_stream:
                stream_cfg = (provider_cfg.get("response") or {}).get("stream") or {}
                if not stream_cfg:
                    print_error(
                        f"Provider {provider!r}: response.stream config missing; "
                        f"cannot decode SSE for this API. Add a response.stream block to the API definition."
                    )
                    return 1, None, False
                final = _stream_sse_response(resp, stream_cfg, suppress_text=raw_mode)
                return 0, final, True
            raw = resp.read().decode("utf-8")
            try:
                return 0, json.loads(raw), False
            except Exception:
                print(raw)
                return 0, None, False
    except urllib.error.HTTPError as exc:
        detail = ""
        try:
            detail_body = exc.read().decode("utf-8")
            if detail_body:
                detail = f": {detail_body}"
        except Exception:
            pass
        print_error(f"HTTP error ({exc.code}) from provider: {exc.reason}{detail}")
        return exc.code or 1, None, False
    except urllib.error.URLError as exc:
        print_error(f"Network error: {exc}")
        return 1, None, False
    except Exception as exc:
        print_error(f"Unexpected error: {exc}")
        return 1, None, False


# ---------------------------------------------------------------------------
# Schema-driven CLI flag parsing for `respond` and `prompts set`
# ---------------------------------------------------------------------------

_BUILTIN_RESPOND_FLAGS = {
    "--model", "--raw", "-r", "--verbose", "-v", "--no-validate",
    "--set", "--unset", "--tool", "--params", "--cite",
}


def _normalize_flag(tok: str) -> str:
    """Normalize a flag for lookup: ``--max_tokens`` and ``--max-tokens`` are equivalent."""
    if tok.startswith("--"):
        return "--" + tok[2:].replace("_", "-")
    return tok


def _coerce_set_value(raw: str) -> object:
    """Coerce a ``--set`` value with type detection.

    - ``:json:<expr>`` forces JSON parsing.
    - ``true``/``false``/``null`` are coerced to bool/None.
    - Strings parsable as int/float become numbers.
    - Strings starting with ``{`` or ``[`` are parsed as JSON.
    - Everything else stays a string.
    """
    if raw.startswith(":json:"):
        return json.loads(raw[len(":json:"):])
    if raw in ("true", "True"):
        return True
    if raw in ("false", "False"):
        return False
    if raw in ("null", "None"):
        return None
    try:
        return int(raw)
    except ValueError:
        pass
    try:
        return float(raw)
    except ValueError:
        pass
    stripped = raw.lstrip()
    if stripped.startswith(("{", "[")):
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            pass
    return raw


def _parse_set_arg(raw: str) -> tuple[str, object]:
    """Parse ``path=value`` into ``(path, coerced_value)``."""
    if "=" not in raw:
        raise ValueError(f"--set requires path=value (got {raw!r})")
    path, _, value_str = raw.partition("=")
    path = path.strip()
    if not path:
        raise ValueError(f"--set path is empty in {raw!r}")
    return path, _coerce_set_value(value_str)


def _coerce_typed_value(
    raw: str, schema_type: object, enum: list | None, enum_map: dict | None
) -> object:
    """Coerce a CLI string value to the schema's expected type."""
    if isinstance(enum_map, dict) and raw in enum_map:
        return enum_map[raw]
    if isinstance(schema_type, list):
        for t in schema_type:
            try:
                return _coerce_typed_value(raw, t, None, None)
            except ValueError:
                continue
        raise ValueError(f"could not coerce {raw!r} to any of {schema_type!r}")
    if schema_type == "integer":
        try:
            return int(raw)
        except ValueError:
            raise ValueError(f"expected integer, got {raw!r}") from None
    if schema_type == "number":
        try:
            return float(raw)
        except ValueError:
            raise ValueError(f"expected number, got {raw!r}") from None
    if schema_type == "boolean":
        if raw in ("true", "True", "1", "yes"):
            return True
        if raw in ("false", "False", "0", "no"):
            return False
        raise ValueError(f"expected boolean, got {raw!r}")
    if schema_type in ("object", "array"):
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            raise ValueError(f"expected JSON {schema_type}, got {raw!r}") from None
    if isinstance(enum, list) and raw not in enum:
        # enum check only when there's no enum_map mapping
        raise ValueError(f"value {raw!r} not in enum {enum!r}")
    return raw


def _parse_respond_flags(
    flag_specs: list[dict], presets_available: list[str], tokens: list[str], *, start: int
) -> tuple[dict | None, str | None]:
    """Parse respond's CLI tokens into a structured plan, driven by the schema.

    Returns ``(parsed, error)``. On success ``error`` is None and ``parsed`` has:
      ``set_paths``, ``unset_paths``, ``tool_presets_selected``, ``model_override``,
      ``raw``, ``verbose``, ``no_validate``, ``params_file``.
    """
    flag_map: dict[str, dict] = {}
    for spec in flag_specs:
        for key in (spec.get("flag"), spec.get("alias")):
            if key:
                flag_map[_normalize_flag(key)] = spec

    set_paths: list[tuple[str, object]] = []
    unset_paths: list[str] = []
    tool_presets_selected: list[str] = []
    model_override: str | None = None
    raw = False
    verbose = False
    no_validate = False
    cite = False
    params_file: str | None = None

    idx = start
    while idx < len(tokens):
        raw_tok = tokens[idx]
        tok = _normalize_flag(raw_tok)
        if tok == "--model":
            if idx + 1 >= len(tokens):
                return None, "--model requires a value"
            model_override = tokens[idx + 1]
            idx += 2
            continue
        if tok in {"--raw", "-r"}:
            raw = True
            idx += 1
            continue
        if tok in {"--verbose", "-v"}:
            verbose = True
            idx += 1
            continue
        if tok == "--no-validate":
            no_validate = True
            idx += 1
            continue
        if tok == "--cite":
            cite = True
            idx += 1
            continue
        if tok == "--set":
            if idx + 1 >= len(tokens):
                return None, "--set requires path=value"
            try:
                path, val = _parse_set_arg(tokens[idx + 1])
            except ValueError as exc:
                return None, str(exc)
            set_paths.append((path, val))
            idx += 2
            continue
        if tok == "--unset":
            if idx + 1 >= len(tokens):
                return None, "--unset requires a path"
            unset_paths.append(tokens[idx + 1])
            idx += 2
            continue
        if tok == "--tool":
            if idx + 1 >= len(tokens):
                return None, "--tool requires a preset name"
            for name in tokens[idx + 1].split(","):
                name = name.strip()
                if not name:
                    continue
                if name not in presets_available:
                    return None, (
                        f"--tool: unknown preset {name!r} "
                        f"(available: {sorted(presets_available)})"
                    )
                tool_presets_selected.append(name)
            idx += 2
            continue
        if tok == "--params":
            if idx + 1 >= len(tokens):
                return None, "--params requires a file path"
            params_file = tokens[idx + 1]
            idx += 2
            continue
        if tok in flag_map:
            spec = flag_map[tok]
            schema_type = spec.get("type")
            path = spec.get("path")
            if schema_type == "boolean":
                set_paths.append((path, True))
                idx += 1
                continue
            if spec.get("repeat"):
                if idx + 1 >= len(tokens):
                    return None, f"{tok} requires a value"
                existing: list | None = None
                for p, v in set_paths:
                    if p == path and isinstance(v, list):
                        existing = v
                        break
                if existing is None:
                    existing = []
                    set_paths.append((path, existing))
                existing.append(tokens[idx + 1])
                idx += 2
                continue
            if idx + 1 >= len(tokens):
                return None, f"{tok} requires a value"
            try:
                val = _coerce_typed_value(
                    tokens[idx + 1], schema_type, spec.get("enum"), spec.get("enum_map")
                )
            except ValueError as exc:
                return None, f"{tok}: {exc}"
            set_paths.append((path, val))
            idx += 2
            continue
        return None, f"Unknown option {raw_tok!r}"

    return (
        {
            "set_paths": set_paths,
            "unset_paths": unset_paths,
            "tool_presets_selected": tool_presets_selected,
            "model_override": model_override,
            "raw": raw,
            "verbose": verbose,
            "no_validate": no_validate,
            "cite": cite,
            "params_file": params_file,
        },
        None,
    )


def _load_params_file(path: str) -> tuple[dict | None, str | None]:
    """Load a YAML or JSON overrides file. Returns ``(data, error)``."""
    try:
        with open(path) as fh:
            text = fh.read()
    except OSError as exc:
        return None, f"Cannot read --params file {path!r}: {exc}"
    if path.endswith(".json"):
        try:
            data = json.loads(text)
        except json.JSONDecodeError as exc:
            return None, f"--params: invalid JSON in {path!r}: {exc}"
    else:
        if not yaml:
            return None, "--params with .yaml requires PyYAML; install with `pip install pyyaml`."
        try:
            data = yaml.safe_load(text)
        except yaml.YAMLError as exc:
            return None, f"--params: invalid YAML in {path!r}: {exc}"
    if not isinstance(data, dict):
        return None, f"--params file {path!r} must contain a top-level dict."
    return data, None


def render_response_output(
    data: dict | None,
    *,
    raw: bool,
    provider_cfg: dict | None = None,
    cite: bool = False,
    print_text: bool = True,
) -> None:
    if data is None:
        if print_text:
            print_error("No response payload.")
        return
    if raw:
        print(json.dumps(data, indent=2))
        return
    resp_cfg = (provider_cfg or {}).get("response", {})
    text_extract_cfg = resp_cfg.get("text_extract")
    if print_text:
        text = lexi.extract_text(data, text_extract_cfg)
        if text:
            print(text)
        else:
            # Schema returned nothing matchable — surface the raw payload so
            # the user can see what came back instead of silent failure.
            print(json.dumps(data, indent=2))
    if cite:
        annotations_cfg = resp_cfg.get("annotations")
        if annotations_cfg:
            text_items = lexi.find_text_items(data, text_extract_cfg)
            anns = lexi.extract_annotations(data, annotations_cfg, text_items=text_items)
            rendered = lexi.render_annotations(anns, annotations_cfg)
            if rendered:
                print()
                print("Sources:")
                for line in rendered.split("\n"):
                    print(f"  {line}")


def read_alias_file(path: Path) -> dict:
    try:
        data = json.loads(path.read_text())
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def print_error(message: str) -> None:
    print(f"{RED}{message}{RESET}")


def print_pretty_json_if_possible(text: str) -> bool:
    try:
        data = json.loads(text)
        if Console:
            console.print_json(json.dumps(data))
        else:
            print(json.dumps(data, indent=2))
        return True
    except Exception:
        return False


def parse_default_prompt_settings() -> dict:
    """Read defaults from config/config.yaml if available."""
    defaults_path = REPO_ROOT / "config" / "config.yaml"
    if not defaults_path.exists():
        defaults_path = REPO_ROOT / "config" / "config.yaml.example"
    if not defaults_path.exists():
        return {}

    try:
        import yaml  # type: ignore
    except Exception:
        # Fallback to a minimal parser for the defaults section.
        try:
            text = defaults_path.read_text()
        except OSError:
            return {}
        defaults: dict[str, str | float | int] = {}
        in_defaults = False
        for raw_line in text.splitlines():
            line = raw_line.rstrip("\n")
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.startswith("defaults:"):
                in_defaults = True
                continue
            if not in_defaults:
                continue
            if not line.startswith("  "):
                break  # left defaults block
            if ":" not in stripped:
                continue
            key, value = stripped.split(":", 1)
            value = value.strip().strip('"').strip("'")
            if not value:
                continue
            try:
                if "." in value:
                    parsed: float | int | str = float(value)
                else:
                    parsed = int(value)
            except ValueError:
                parsed = value
            defaults[key.strip()] = parsed
        return defaults

    try:
        data = yaml.safe_load(defaults_path.read_text()) or {}
        defaults = data.get("defaults", {}) if isinstance(data, dict) else {}
        return defaults if isinstance(defaults, dict) else {}
    except Exception:
        return {}


def ensure_data_dir() -> None:
    """Create the app data dir and migrate legacy files if present."""
    try:
        APP_DIR.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        print_error(f"Warning: could not prepare data dir ({exc}).")
        return

    def migrate(src: Path, dest: Path) -> None:
        if dest.exists() or not src.exists():
            return
        try:
            dest.write_text(src.read_text())
        except OSError as exc:
            print_error(f"Warning: could not migrate {src.name} ({exc}).")

    migrate(LEGACY_CONFIG_FILE, CONFIG_FILE)
    migrate(LEGACY_HISTORY_FILE, HISTORY_FILE)
    migrate(LEGACY_APP_DIR / "config.json", CONFIG_FILE)
    migrate(LEGACY_APP_DIR / "history", HISTORY_FILE)
    # migrate legacy prompts.json into per-file prompts
    legacy_prompts_file = LEGACY_APP_DIR / "prompts.json"
    if legacy_prompts_file.exists() and not PROMPTS_DIR.exists():
        try:
            PROMPTS_DIR.mkdir(parents=True, exist_ok=True)
            legacy_data = json.loads(legacy_prompts_file.read_text())
            if isinstance(legacy_data, dict):
                for name, payload in legacy_data.items():
                    if isinstance(payload, dict) and name:
                        (PROMPTS_DIR / f"{name}.json").write_text(json.dumps(payload, indent=2) + "\n")
        except Exception as exc:
            print_error(f"Warning: could not migrate legacy prompts ({exc}).")

    seed_default_providers()


def run_command(command: str) -> int:
    """Run a shell command and stream its output."""
    if not command.strip():
        return 0
    try:
        completed = subprocess.run(command, shell=True)
        return completed.returncode
    except KeyboardInterrupt:
        return 130


def load_history() -> None:
    if not HISTORY_FILE.exists():
        return
    try:
        for line in HISTORY_FILE.read_text().splitlines():
            trimmed = line.strip()
            if trimmed:
                HISTORY.append(trimmed)
    except OSError as exc:
        print_error(f"Warning: could not read history ({exc}).")


def save_history() -> None:
    try:
        HISTORY_FILE.write_text("\n".join(HISTORY) + ("\n" if HISTORY else ""))
    except OSError as exc:
        print_error(f"Warning: could not persist history ({exc}).")


def load_prompts() -> None:
    PROMPTS.clear()
    if not PROMPTS_DIR.exists():
        return
    try:
        for entry in PROMPTS_DIR.glob("*.json"):
            try:
                data = json.loads(entry.read_text())
                if isinstance(data, dict):
                    PROMPTS[entry.stem] = data
            except (OSError, json.JSONDecodeError):
                continue
    except OSError as exc:
        print_error(f"Warning: could not read prompts ({exc}).")


def save_prompts() -> None:
    try:
        PROMPTS_DIR.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        print_error(f"Warning: could not prepare prompts directory ({exc}).")
        return

    # Persist each prompt to its own file
    existing = set()
    for name, payload in PROMPTS.items():
        if not isinstance(payload, dict):
            continue
        path = PROMPTS_DIR / f"{name}.json"
        existing.add(path.name)
        try:
            path.write_text(json.dumps(payload, indent=2) + "\n")
        except OSError as exc:
            print_error(f"Warning: could not save prompt {name!r} ({exc}).")

    # Clean up stale files
    try:
        for entry in PROMPTS_DIR.glob("*.json"):
            if entry.name not in existing:
                entry.unlink(missing_ok=True)
    except OSError:
        pass


def seed_default_providers() -> None:
    """Copy default providers and models into the app dir if missing."""
    if not DEFAULT_PROVIDERS_CONFIG.exists():
        return
    try:
        PROVIDERS_ROOT.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        print_error(f"Warning: could not prepare providers dir ({exc}).")
        return

    try:
        default_text = DEFAULT_PROVIDERS_CONFIG.read_text()
    except OSError:
        default_text = ""

    if default_text:
        try:
            needs_update = True
            if PROVIDERS_CONFIG.exists():
                try:
                    if PROVIDERS_CONFIG.read_text() == default_text:
                        needs_update = False
                    else:
                        backup = PROVIDERS_CONFIG.with_suffix(PROVIDERS_CONFIG.suffix + ".bak")
                        shutil.copyfile(PROVIDERS_CONFIG, backup)
                except OSError:
                    pass
            if needs_update:
                shutil.copyfile(DEFAULT_PROVIDERS_CONFIG, PROVIDERS_CONFIG)
        except OSError as exc:
            print_error(f"Warning: could not seed providers config ({exc}).")

    # Seed default aliases if missing or empty
    if DEFAULT_ALIASES_FILE and DEFAULT_ALIASES_FILE.exists():
        should_seed = False
        if not ALIASES_FILE.exists():
            should_seed = True
        else:
            existing_aliases = read_alias_file(ALIASES_FILE)
            if not existing_aliases:
                should_seed = True
        if should_seed:
            try:
                ALIASES_FILE.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(DEFAULT_ALIASES_FILE, ALIASES_FILE)
            except OSError as exc:
                print_error(f"Warning: could not seed aliases ({exc}).")

    for entry in DEFAULT_PROVIDERS_ROOT.iterdir() if DEFAULT_PROVIDERS_ROOT.exists() else []:
        if not entry.is_dir():
            continue
        dest_dir = PROVIDERS_ROOT / entry.name
        dest_dir.mkdir(parents=True, exist_ok=True)

        src_models = entry / "configured-models.yaml"
        if not src_models.exists():
            src_models = entry / "models.yaml"
        dest_models = dest_dir / "configured-models.yaml"
        if src_models.exists() and not dest_models.exists():
            try:
                shutil.copyfile(src_models, dest_models)
            except OSError as exc:
                print_error(f"Warning: could not seed models for {entry.name} ({exc}).")

        src_aliases = entry / "model-aliases.yaml"
        dest_aliases = dest_dir / "model-aliases.yaml"
        if src_aliases.exists() and not dest_aliases.exists():
            try:
                shutil.copyfile(src_aliases, dest_aliases)
            except OSError as exc:
                print_error(f"Warning: could not seed model aliases for {entry.name} ({exc}).")

        src_default = entry / "default-model.yaml"
        dest_default = dest_dir / "default-model.yaml"
        if src_default.exists() and not dest_default.exists():
            try:
                shutil.copyfile(src_default, dest_default)
            except OSError as exc:
                print_error(f"Warning: could not seed default model for {entry.name} ({exc}).")

        src_api_key = entry / "api-key"
        dest_api_key = dest_dir / "api-key"
        if src_api_key.exists() and not dest_api_key.exists():
            try:
                shutil.copyfile(src_api_key, dest_api_key)
                dest_api_key.chmod(0o600)
            except OSError as exc:
                print_error(f"Warning: could not seed api-key for {entry.name} ({exc}).")

    seed_default_apis()


def load_providers_config() -> dict:
    data = load_yaml_file(PROVIDERS_CONFIG)
    data = expand_env_vars(data)
    return data if isinstance(data, dict) else {}


def save_providers_config(data: dict) -> bool:
    return save_yaml_file(PROVIDERS_CONFIG, data)


def provider_models_path(name: str) -> Path:
    return PROVIDERS_ROOT / name / "configured-models.yaml"


def provider_default_model_path(name: str) -> Path:
    return PROVIDERS_ROOT / name / "default-model.yaml"


def provider_model_aliases_path(name: str) -> Path:
    return PROVIDERS_ROOT / name / "model-aliases.yaml"


def provider_api_key_path(name: str) -> Path:
    """Return the path to the plain-text api-key file for a provider."""
    return PROVIDERS_ROOT / name / "api-key"


def load_api_key(name: str, provider_cfg: dict) -> str:
    """Load API key with priority: api-key file > env var in config > literal in config."""
    key_file = provider_api_key_path(name)
    if key_file.exists():
        try:
            return key_file.read_text().strip()
        except OSError:
            pass
    return provider_cfg.get("api_key") or ""


def check_api_key_valid(name: str, provider_cfg: dict, api_key: str) -> bool | None:
    """Validate an API key by hitting the hosted-models endpoint.

    Returns True (2xx), False (401/403), or None (can't determine).
    """
    base_url = provider_cfg.get("url", "").rstrip("/")
    hosted_path = provider_cfg.get("hosted_models", "")
    if not base_url or not hosted_path:
        return None
    endpoint = f"{base_url}{hosted_path}" if hosted_path.startswith("/") else f"{base_url}/{hosted_path}"
    effective_cfg = resolve_effective_config(name, provider_cfg)
    headers = build_auth_headers(effective_cfg, api_key)
    try:
        req = urllib.request.Request(endpoint, headers=headers)  # type: ignore[arg-type]
        with urllib.request.urlopen(req, timeout=5) as resp:  # type: ignore[call-arg]
            return resp.status < 400
    except urllib.error.HTTPError as exc:
        if exc.code in (401, 403):
            return False
        return None
    except Exception:
        return None


def get_api_key_status(name: str, provider_cfg: dict, *, validate: bool = False) -> dict:
    """Return API key info: ``{"key": str, "source": str, "status": str}``.

    *source* is ``"file"``, ``"env"``, or ``""`` (none).
    *status* is ``"ready"``, ``"no api-key"``, or ``"invalid api-key"``.
    When *validate* is True, makes an HTTP call to check key validity.
    """
    import re

    key = ""
    source = ""

    # 1. Check api-key file
    key_file = provider_api_key_path(name)
    if key_file.exists():
        try:
            contents = key_file.read_text().strip()
            if contents:
                key = contents
                source = "file"
        except OSError:
            pass

    # 2. Check config value (raw, without env expansion)
    if not key:
        raw_data = load_yaml_file(PROVIDERS_CONFIG)
        raw_providers = raw_data.get("providers", {}) if isinstance(raw_data, dict) else {}
        raw_cfg = raw_providers.get(name, {}) if isinstance(raw_providers, dict) else {}
        raw_val = raw_cfg.get("api_key", "") if isinstance(raw_cfg, dict) else ""
        if isinstance(raw_val, str) and raw_val:
            env_match = re.match(r"^\$\{(.+)\}$", raw_val)
            if env_match:
                env_val = os.environ.get(env_match.group(1), "")
                if env_val:
                    key = env_val
                    source = "env"
            else:
                key = raw_val
                source = "file"

    if not key:
        return {"key": "", "source": "", "status": "no api-key"}

    status = "ready"
    if validate:
        valid = check_api_key_valid(name, provider_cfg, key)
        if valid is False:
            status = "invalid api-key"
    return {"key": key, "source": source, "status": status}


def save_api_key(name: str, key: str) -> bool:
    """Write API key to file with chmod 0o600."""
    key_file = provider_api_key_path(name)
    try:
        key_file.parent.mkdir(parents=True, exist_ok=True)
        key_file.write_text(key + "\n")
        key_file.chmod(0o600)
        return True
    except OSError as exc:
        print_error(f"Could not save API key: {exc}")
        return False


def remove_api_key(name: str) -> bool:
    """Delete the api-key file for a provider."""
    key_file = provider_api_key_path(name)
    if not key_file.exists():
        return False
    try:
        key_file.unlink()
        return True
    except OSError as exc:
        print_error(f"Could not remove API key: {exc}")
        return False


def load_models(provider: str) -> dict:
    path = provider_models_path(provider)
    data = load_yaml_file(path)
    if data:
        return data
    legacy = PROVIDERS_ROOT / provider / "models.yaml"
    if legacy.exists():
        return load_yaml_file(legacy)
    return {}


def save_models(provider: str, data: dict) -> bool:
    return save_yaml_file(provider_models_path(provider), data)


def load_default_model(provider: str) -> str | None:
    data = load_yaml_file(provider_default_model_path(provider))
    if isinstance(data, dict):
        return data.get("default_model")
    if isinstance(data, str):
        return data
    return None


def save_default_model(provider: str, model: str | None) -> bool:
    if model is None:
        return save_yaml_file(provider_default_model_path(provider), {})
    return save_yaml_file(provider_default_model_path(provider), {"default_model": model})


def load_model_aliases(provider: str) -> dict:
    data = load_yaml_file(provider_model_aliases_path(provider))
    if not isinstance(data, dict):
        return {}
    if "aliases" in data and isinstance(data["aliases"], dict):
        return data["aliases"]
    return data


def save_model_aliases(provider: str, aliases: dict) -> bool:
    """Save model aliases, preserving the file's existing wrapper format.

    If the raw file uses an ``aliases:`` key, wrap the data; otherwise save flat.
    """
    path = provider_model_aliases_path(provider)
    wrapped = False
    if path.exists():
        try:
            raw = load_yaml_file(path)
            if isinstance(raw, dict) and "aliases" in raw:
                wrapped = True
        except Exception:
            pass
    data = {"aliases": aliases} if wrapped else aliases
    return save_yaml_file(path, data)


def load_config() -> None:
    global CONFIG
    if CONFIG_FILE.exists():
        try:
            data = json.loads(CONFIG_FILE.read_text())
            if not isinstance(data, dict):
                raise ValueError("Config file must contain a JSON object.")
            # Remove legacy 'version' key — version is now read from package metadata
            dirty = "version" in data
            data.pop("version", None)
            CONFIG = {**CONFIG_DEFAULT, **{k: str(v) for k, v in data.items()}}
            if dirty:
                save_config()
            return
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print_error(f"Warning: could not read config ({exc}); using defaults.")

    CONFIG = CONFIG_DEFAULT.copy()
    save_config()


def save_config() -> None:
    try:
        CONFIG_FILE.write_text(json.dumps(CONFIG, indent=2) + "\n")
    except OSError as exc:
        print_error(f"Warning: could not persist config ({exc}).")


def enable_line_editing() -> None:
    """Configure readline for vi/emacs editing and prime history, if available."""
    if readline is None:
        return

    requested_mode = CONFIG.get("edit_mode", CONFIG_DEFAULT["edit_mode"]).lower()
    if requested_mode not in {"vi", "emacs"}:
        requested_mode = CONFIG_DEFAULT["edit_mode"]

    bindings: Iterable[str] = [
        "set editing-mode vi" if requested_mode == "vi" else "set editing-mode emacs",
        '"\\e[A": previous-history',
        '"\\e[B": next-history',
    ]

    for binding in bindings:
        try:
            readline.parse_and_bind(binding)
        except Exception:
            continue

    try:
        readline.clear_history()
        for entry in HISTORY:
            readline.add_history(entry)
    except Exception:
        return


def prompt_defaults() -> dict:
    """Return prompt defaults, optionally overridden by config/config.yaml defaults."""
    global PROMPT_DEFAULTS_LOADED, PROMPT_RESOLVED_DEFAULT
    if PROMPT_DEFAULTS_LOADED:
        return PROMPT_RESOLVED_DEFAULT.copy()
    base = PROMPT_DEFAULT.copy()
    overrides = parse_default_prompt_settings()
    if overrides:
        for key in ("max_tokens", "temperature", "role"):
            if key in overrides:
                base[key] = overrides[key]
    PROMPT_DEFAULTS_LOADED = True
    PROMPT_RESOLVED_DEFAULT = base.copy()
    return base.copy()


def print_config_values(keys: list[str] | None = None) -> None:
    items = [(k, CONFIG[k]) for k in keys if k in CONFIG] if keys else list(CONFIG.items())
    if not items:
        print("No config values set.")
        return

    for key, value in items:
        print(f'{key}="{value}"')


def print_providers(data: dict, *, table: bool = False) -> None:
    providers = data.get("providers") if isinstance(data, dict) else None
    if not providers:
        print("No providers configured.")
        return

    if table and (Console is None or Table is None or console is None):
        print_error("Rich is not available; falling back to plain output.")
        table = False

    if table and console:
        table_obj = Table(title="Providers")
        table_obj.add_column("Provider")
        table_obj.add_column("URL")
        table_obj.add_column("API")
        table_obj.add_column("Key Source")
        table_obj.add_column("Status")
        for name, cfg in providers.items():
            url = cfg.get("url", "")
            api_name = cfg.get("api", "")
            info = get_api_key_status(name, cfg, validate=True)
            source = info["source"] or "\u2014"
            status = info["status"]
            if status == "ready":
                status_text = f"[green]{status}[/green]"
            elif status == "invalid api-key":
                status_text = f"[yellow]{status}[/yellow]"
            else:
                status_text = f"[red]{status}[/red]"
            table_obj.add_row(name, url, api_name, source, status_text)
        console.print(table_obj)
        return

    for name, cfg in providers.items():
        url = cfg.get("url", "")
        hosted_models = cfg.get("hosted_models", "")
        hosted_fields = cfg.get("hosted_model_fields", [])
        info = get_api_key_status(name, cfg)
        source = info["source"]
        source_label = f"[{source}]" if source else "[missing]"
        fields = ", ".join(hosted_fields) if isinstance(hosted_fields, list) else hosted_fields
        print(f"{name}: url={url} hosted={hosted_models} fields=[{fields}] api_key={source_label}")


def print_models(
    provider: str, models_data: dict, aliases_data: dict | None = None, *, table: bool = False, details: bool = False
) -> None:
    default_model = load_default_model(provider)
    models_data_dict = models_data if isinstance(models_data, dict) else {}
    models = models_data_dict.get("models", {}) if isinstance(models_data_dict.get("models", {}), dict) else {}
    aliases = aliases_data if isinstance(aliases_data, dict) else {}
    alias_lookup: dict[str, list[str]] = {}
    for alias, target in aliases.items():
        if isinstance(target, str):
            alias_lookup.setdefault(target, []).append(alias)

    if table and (Console is None or Table is None or console is None):
        print_error("Rich is not available; falling back to plain output.")
        table = False

    if not models:
        print(f"No models configured for {provider}.")
        return

    if table and console:
        table_obj = Table(title=f"Models for {provider}")
        table_obj.add_column("Model Name")
        table_obj.add_column("Alias")
        table_obj.add_column("Description")
        for key, meta in models.items():
            meta_dict = meta if isinstance(meta, dict) else {}
            name = meta_dict.get("name") or key
            model_label = f"{name} (default)" if key == default_model else name
            alias_text = ", ".join(sorted(alias_lookup.get(key, [])))
            desc_lines = []
            desc = meta_dict.get("description") or ""
            if desc:
                desc_lines.append(desc)
            if details:
                for detail_key, detail_val in sorted(meta_dict.items()):
                    if detail_key in {"name", "description"}:
                        continue
                    desc_lines.append(f"{detail_key}: {detail_val}")
            description = "\n".join(desc_lines)
            table_obj.add_row(model_label, alias_text, description)
        console.print(table_obj)
        return

    print(f"Models for {provider}:")
    for key, meta in models.items():
        meta_dict = meta if isinstance(meta, dict) else {}
        name = meta_dict.get("name", "")
        desc = meta_dict.get("description", "")
        extra = []
        if key == default_model:
            extra.append("default")
        aliases_for_model = alias_lookup.get(key, [])
        if aliases_for_model:
            extra.append(f"alias: {', '.join(sorted(aliases_for_model))}")
        info = " ".join(extra)
        suffix = f" ({info})" if info else ""
        print(f"  {key}: {name} - {desc}{suffix}")
        if details:
            for detail_key, detail_val in sorted(meta_dict.items()):
                if detail_key in {"name", "description"}:
                    continue
                print(f"    {detail_key}: {detail_val}")
    if aliases:
        print("Aliases:")
        for alias, target in aliases.items():
            print(f"  {alias} -> {target}")


def fetch_hosted_models(provider: str, provider_cfg: dict) -> dict | list | None:
    base_url = (provider_cfg.get("url") or "").rstrip("/")
    # Normalize to base /v1 if someone stored /v1/messages, etc.
    if "/v1/" in base_url:
        base_url = base_url.split("/v1/", 1)[0] + "/v1"
    elif base_url.endswith("/v1/messages"):
        base_url = base_url[: -len("/messages")]
    hosted_path = provider_cfg.get("hosted_models", "/models")
    api_key = load_api_key(provider, provider_cfg)
    if not base_url:
        print_error(f"Provider {provider!r} is missing a url in providers-config.yaml")
        return None
    if not api_key:
        print_error(f"Provider {provider!r} is missing an api_key in providers-config.yaml")
        return None
    if not hosted_path:
        print_error(f"Provider {provider!r} is missing hosted_models path in providers-config.yaml")
        return None
    endpoint = f"{base_url}{hosted_path}" if hosted_path.startswith("/") else f"{base_url}/{hosted_path}"

    effective_cfg = resolve_effective_config(provider, provider_cfg)
    headers = build_auth_headers(effective_cfg, api_key)

    try:
        req = urllib.request.Request(endpoint, headers=headers)  # type: ignore[arg-type]
        with urllib.request.urlopen(req, timeout=15) as resp:  # type: ignore[call-arg]
            body = resp.read()
    except urllib.error.HTTPError as exc:
        detail = ""
        try:
            detail_body = exc.read().decode("utf-8")
            if detail_body:
                detail = f": {detail_body}"
        except Exception:
            pass
        print_error(f"HTTP error fetching hosted models from {provider}: {exc}{detail}")
        return None
    except urllib.error.URLError as exc:
        print_error(f"Network error fetching hosted models from {provider}: {exc}")
        return None
    except Exception as exc:
        print_error(f"Unexpected error fetching hosted models from {provider}: {exc}")
        return None

    try:
        return json.loads(body.decode("utf-8"))
    except Exception:
        print_error("Could not parse hosted models response as JSON.")
        return None


def normalize_hosted_items(data: dict | list | None) -> list:
    if isinstance(data, dict):
        if isinstance(data.get("data"), list):
            return data["data"]
        if isinstance(data.get("models"), list):
            return data["models"]
    if isinstance(data, list):
        return data
    return []


def print_hosted_models(provider: str, provider_cfg: dict, data: dict | list | None, *, raw: bool, table: bool) -> None:
    if data is None:
        print_error("No data returned.")
        return
    if raw:
        print(json.dumps(data, indent=2))
        return

    items = normalize_hosted_items(data)
    if not items:
        print(f"No hosted models returned for {provider}.")
        return

    fields = provider_cfg.get("hosted_model_fields")
    if not isinstance(fields, list) or not fields:
        fields = list(items[0].keys()) if isinstance(items[0], dict) else ["id"]

    if table and console:
        table_obj = Table(title=f"Hosted models for {provider}")
        for field in fields:
            table_obj.add_column(field)
        for item in items:
            if isinstance(item, dict):
                row = [str(item.get(field, "")) for field in fields]
            else:
                row = [str(item) if idx == 0 else "" for idx, _ in enumerate(fields)]
            table_obj.add_row(*row)
        console.print(table_obj)
        return

    print(f"Hosted models for {provider}:")
    for item in items:
        if isinstance(item, dict):
            values = [f"{field}={item.get(field, '')}" for field in fields]
            print(f"  {', '.join(values)}")
        else:
            print(f"  {item}")


def record_history(entry: str) -> None:
    trimmed = entry.strip()
    if trimmed:
        HISTORY.append(trimmed)
        if readline is not None:
            import contextlib

            with contextlib.suppress(Exception):
                readline.add_history(trimmed)
        save_history()


def print_command_with_subcommands(name: str, desc: str) -> None:
    print(f"  {name:<12} - {desc}")
    subcommands = COMMAND_SUBCOMMANDS.get(name, {})
    for sub_name, meta in subcommands.items():
        summary = meta.get("summary", "")
        print(f"    {sub_name:<10} {summary}")


def parse_prompt_options(tokens: list[str], base: dict, *, require_prompt: bool) -> tuple[dict | None, str | None]:
    prompt_parts: list[str] = []
    updated = base.copy()
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if tok in {"--max_tokens", "-m"}:
            if i + 1 >= len(tokens):
                return None, "max_tokens requires an integer value"
            try:
                updated["max_tokens"] = int(tokens[i + 1])
            except ValueError:
                return None, "max_tokens must be an integer"
            i += 2
            continue
        if tok in {"--temperature", "-t"}:
            if i + 1 >= len(tokens):
                return None, "temperature requires a numeric value"
            try:
                updated["temperature"] = float(tokens[i + 1])
            except ValueError:
                return None, "temperature must be numeric"
            i += 2
            continue
        if tok in {"--role", "-r"}:
            if i + 1 >= len(tokens):
                return None, "role requires a value"
            updated["role"] = tokens[i + 1]
            i += 2
            continue
        if tok == "--prompt":
            if i + 1 >= len(tokens):
                return None, "prompt requires a value"
            prompt_parts.append(tokens[i + 1])
            i += 2
            continue
        if tok == "--set":
            if i + 1 >= len(tokens):
                return None, "--set requires path=value"
            try:
                path, value = _parse_set_arg(tokens[i + 1])
            except ValueError as exc:
                return None, str(exc)
            try:
                api_compose.set_path(updated, path, value)
            except (TypeError, ValueError) as exc:
                return None, f"--set: {exc}"
            i += 2
            continue
        if tok == "--unset":
            if i + 1 >= len(tokens):
                return None, "--unset requires a path"
            api_compose.unset_path(updated, tokens[i + 1])
            i += 2
            continue
        prompt_parts.append(tok)
        i += 1

    prompt_text = " ".join(prompt_parts).strip()
    if prompt_text:
        # Support escaped newlines in shell input (e.g., "\n") and preserve literal newlines.
        prompt_text = prompt_text.replace("\\n", "\n")
        updated["prompt"] = prompt_text
    if require_prompt and not updated.get("prompt"):
        return None, "prompt text is required"
    return updated, None


def run_history_entry(index: int) -> tuple[int, bool]:
    if index == 0:
        print_error("History index 0 is invalid; use positive or negative numbers.")
        return 1, False

    resolved_index = index if index >= 0 else len(HISTORY) + index + 1

    if resolved_index <= 0 or resolved_index > len(HISTORY):
        print_error(f"History index {index} is out of range.")
        return 1, False

    target = HISTORY[resolved_index - 1].strip()
    if target.startswith("#"):
        print_error("Cannot re-run a history reference (#...) from history.")
        return 1, False

    return execute_cli_line(target, record=True)


def print_history(count: int | None) -> None:
    if not HISTORY:
        print("History is empty.")
        return

    if count is None or count == 0:
        items = HISTORY
        start_idx = 1
    elif count > 0:
        items = HISTORY[-count:]
        start_idx = len(HISTORY) - len(items) + 1
    else:
        items = HISTORY[: abs(count)]
        start_idx = 1

    for idx, cmd in enumerate(items, start=start_idx):
        print(f"{idx}: {cmd}")


def handle_history(rest: str | None) -> int:
    reset = False
    count: int | None = None
    if rest:
        tokens = shlex.split(rest)
        i = 0
        while i < len(tokens):
            tok = tokens[i]
            if tok in {"--reset", "--clear"}:
                reset = True
                i += 1
                continue
            if tok in {"-n", "--number"}:
                if i + 1 >= len(tokens):
                    print_error("history: -n requires an integer count.")
                    return 1
                try:
                    count = int(tokens[i + 1])
                except ValueError:
                    print_error("history: -n requires an integer count.")
                    return 1
                i += 2
                continue
            print_error(f"history: unknown argument {tok!r}. Use -n COUNT, --reset, or --clear.")
            return 1

    if reset:
        HISTORY.clear()
        save_history()
        if readline is not None:
            import contextlib

            with contextlib.suppress(Exception):
                readline.clear_history()
        print("History cleared.")
        if count is None:
            return 0

    print_history(count)
    return 0


def handle_config(rest: str | None, tokens_override: list[str] | None = None) -> int:
    if not rest and tokens_override is None:
        print_error("Usage: config list [--raw|-r|key] | config set <key> <value> | config rm <key>")
        return 1

    tokens = tokens_override if tokens_override is not None else shlex.split(rest)
    if not tokens:
        print_error("Usage: config list [--raw|-r|key] | config set <key> <value> | config rm <key>")
        return 1

    subcommand = tokens[0].lower()
    if subcommand == "list":
        raw_requested = False
        if "--raw" in tokens[1:] or "-r" in tokens[1:]:
            raw_requested = True
            tokens = [t for t in tokens if t not in {"--raw", "-r"}]

        if raw_requested and len(tokens) == 1:
            try:
                raw = CONFIG_FILE.read_text()
            except OSError as exc:
                print_error(f"Could not read config file: {exc}")
                return 1
            if not print_pretty_json_if_possible(raw):
                print(raw, end="" if raw.endswith("\n") else "\n")
            return 0

        # Support list <key> [--value|-v] and list --value|-v <key>
        if len(tokens) in {1, 2, 3}:
            value_only = False
            key = None
            for tok in tokens[1:]:
                if tok in {"--value", "-v"}:
                    value_only = True
                else:
                    key = tok

            if key:
                if key in CONFIG:
                    if value_only:
                        print(CONFIG[key])
                    else:
                        print_config_values([key])
                    return 0
                print_error(f"Config key {key!r} not found.")
                return 1

            if raw_requested:
                try:
                    raw = CONFIG_FILE.read_text()
                except OSError as exc:
                    print_error(f"Could not read config file: {exc}")
                    return 1
                if not print_pretty_json_if_possible(raw):
                    print(raw, end="" if raw.endswith("\n") else "\n")
                return 0

            print_config_values()
            return 0

        print_error("Usage: config list [--raw|-r|key]")
        return 1

    if subcommand == "set":
        if len(tokens) < 3:
            print_error("Usage: config set <key> <value>")
            return 1
        key, value = tokens[1], " ".join(tokens[2:])
        CONFIG[key] = value
        save_config()
        print(f"Set {key} = {value}")
        return 0

    if subcommand == "rm":
        if len(tokens) != 2:
            print_error("Usage: config rm <key>")
            return 1
        key = tokens[1]
        if key not in CONFIG:
            print_error(f"Config key {key!r} not found.")
            return 1
        del CONFIG[key]
        save_config()
        print(f"Removed {key}")
        return 0

    print_error("Usage: config list [--raw|-r|key] | config set <key> <value> | config rm <key>")
    return 1


def handle_prompt(rest: str | None, tokens_override: list[str] | None = None) -> int:
    usage = (
        "prompts <text> [[-m|--max_tokens] N] [[-t|--temperature] T] [[-r|--role] ROLE]\n"
        "prompts list [name] [[-r|--raw] [-d|--detailed] [-t|--table]]\n"
        "prompts set [name] [--prompt TEXT] [[-m|--max_tokens] N] [[-t|--temperature] T] [[-r|--role] ROLE] [--set path=value] [--unset path]\n"
        "prompts rm <name>"
    )
    if not rest:
        print_error(f"Usage:\n{usage}")
        return 1

    tokens = tokens_override if tokens_override is not None else shlex.split(rest)
    if not tokens:
        print_error(f"Usage:\n{usage}")
        return 1

    sub = tokens[0].lower()
    if sub in {"list", "ls"}:
        name: str | None = None
        raw = False
        detailed = False
        use_table = False
        i = 1
        while i < len(tokens):
            tok = tokens[i]
            if tok in {"-r", "--raw"}:
                raw = True
                i += 1
                continue
            if tok in {"-d", "--detailed"}:
                detailed = True
                i += 1
                continue
            if tok in {"-t", "--table"}:
                use_table = True
                i += 1
                continue
            if tok.startswith("-"):
                print_error(f"Unknown flag {tok!r} for prompt list.")
                return 1
            if name is None:
                name = tok
                i += 1
                continue
            print_error("prompt list accepts at most one name.")
            return 1

        def render_prompt(pname: str, payload: dict) -> None:
            prompt_text = payload.get("prompt", "")
            if raw:
                print(json.dumps({pname: payload}, indent=2))
                return
            if use_table and console:
                return  # handled in table rendering path
            if detailed:
                max_tokens = payload.get("max_tokens", "")
                temp = payload.get("temperature", "")
                role = payload.get("role", "")
                prefix = "* " if pname == PROMPT_ACTIVE_NAME else ""
                print(f"{prefix}{pname}: {prompt_text}")
                print(f"    max_tokens={max_tokens} temperature={temp} role={role}")
                return
            prefix = "* " if pname == PROMPT_ACTIVE_NAME else ""
            print(f"{prefix}{pname}: {prompt_text}")

        if name:
            payload = PROMPTS.get(name)
            if not payload:
                print_error(f"Prompt {name!r} not found.")
                return 1
            if use_table and console:
                table_obj = Table(title=f"Prompt {name}")
                table_obj.add_column("Name")
                table_obj.add_column("Prompt")
                table_obj.add_column("max_tokens")
                table_obj.add_column("temperature")
                table_obj.add_column("role")
                table_obj.add_row(
                    name,
                    payload.get("prompt", ""),
                    str(payload.get("max_tokens", "")),
                    str(payload.get("temperature", "")),
                    payload.get("role", ""),
                )
                console.print(table_obj)
            else:
                render_prompt(name, payload)
            return 0

        if not PROMPTS:
            print("No prompts saved.")
            return 0

        if use_table and console:
            table_obj = Table(title="Prompts")
            table_obj.add_column("Name")
            table_obj.add_column("Prompt")
            table_obj.add_column("max_tokens")
            table_obj.add_column("temperature")
            table_obj.add_column("role")
            # Active first, then rest
            ordered = []
            if PROMPT_ACTIVE_NAME in PROMPTS:
                ordered.append((PROMPT_ACTIVE_NAME, PROMPTS[PROMPT_ACTIVE_NAME]))
            for pname, payload in PROMPTS.items():
                if pname == PROMPT_ACTIVE_NAME:
                    continue
                ordered.append((pname, payload))
            for pname, payload in ordered:
                table_obj.add_row(
                    pname,
                    payload.get("prompt", ""),
                    str(payload.get("max_tokens", "")),
                    str(payload.get("temperature", "")),
                    payload.get("role", ""),
                )
            console.print(table_obj)
            return 0

        # List active first if present, then others.
        if PROMPT_ACTIVE_NAME in PROMPTS:
            render_prompt(PROMPT_ACTIVE_NAME, PROMPTS[PROMPT_ACTIVE_NAME])

        for pname, payload in PROMPTS.items():
            if pname == PROMPT_ACTIVE_NAME:
                continue
            render_prompt(pname, payload)
        return 0

    if sub in {"set", "save"}:
        name: str | None = None
        remaining_tokens = tokens[1:]

        if remaining_tokens and not remaining_tokens[0].startswith("-"):
            if len(remaining_tokens) >= 2:
                name = remaining_tokens[0]
                remaining_tokens = remaining_tokens[1:]
            else:
                # Single positional with no flags -> treat as prompt text for active prompt
                remaining_tokens = [remaining_tokens[0]]

        name = name or PROMPT_ACTIVE_NAME
        base = prompt_defaults()
        if name in PROMPTS and isinstance(PROMPTS[name], dict):
            base = {**base, **PROMPTS[name]}
        payload, err = parse_prompt_options(remaining_tokens, base, require_prompt=False)
        if err:
            print_error(err)
            return 1
        if not payload or not payload.get("prompt"):
            print_error("prompt set requires prompt text (use --prompt or supply text).")
            return 1
        PROMPTS[name] = payload
        save_prompts()
        print(f"Saved prompt '{name}'.")
        return 0

    if sub in {"rm", "del", "delete"}:
        if len(tokens) != 2:
            print_error("prompt rm <name>")
            return 1
        name = tokens[1]
        if name not in PROMPTS:
            print_error(f"Prompt {name!r} not found.")
            return 1
        del PROMPTS[name]
        save_prompts()
        print(f"Removed prompt '{name}'.")
        return 0

    # Ad-hoc prompt: build payload from defaults and user-supplied text/options.
    payload, err = parse_prompt_options(tokens, prompt_defaults(), require_prompt=True)
    if err:
        print_error(err)
        return 1
    print(json.dumps(payload, indent=2))
    return 0


def handle_provider(rest: str | None, tokens_override: list[str] | None = None) -> int:
    usage = (
        "providers list [--raw|-r] [--table|-t]\n"
        "providers status <name> [--raw|-r]\n"
        "providers add <name> --url URL --api_key KEY --default_model MODEL\n"
        "providers api-key set <name> [--value KEY]\n"
        "providers api-key show <name> [--reveal]\n"
        "providers api-key remove <name>\n"
        "providers rename <old> <new>\n"
        "providers rm <name>"
    )
    tokens = tokens_override if tokens_override is not None else shlex.split(rest or "")
    if not tokens:
        print_error(f"Usage:\n{usage}")
        return 1
    sub = tokens[0].lower()
    cfg = load_providers_config()
    providers = cfg.setdefault("providers", {})

    # If file missing providers but repo has defaults, hydrate from repo providers-config.yaml
    if not providers and PROVIDERS_CONFIG.exists():
        defaults = load_yaml_file(PROVIDERS_CONFIG)
        if defaults.get("providers"):
            cfg = defaults
            providers = cfg.setdefault("providers", {})

    if sub in {"list", "ls"}:
        args = tokens[1:]
        if args and args[0] in {"-r", "--raw"}:
            try:
                raw = PROVIDERS_CONFIG.read_text()
                if not print_pretty_json_if_possible(raw):
                    print(raw, end="" if raw.endswith("\n") else "\n")
            except OSError as exc:
                print_error(f"Could not read providers config: {exc}")
                return 1
            return 0
        use_table = any(a in {"-t", "--table"} for a in args)
        print_providers(cfg, table=use_table)
        return 0

    if sub == "status":
        args = tokens[1:]
        if not args or args[0].startswith("-"):
            print_error("Usage: providers status <name> [--raw|-r]")
            return 1
        name = args[0]
        if name not in providers:
            print_error(f"Provider {name!r} not found.")
            return 1
        use_raw = any(a in {"-r", "--raw"} for a in args[1:])
        info = get_api_key_status(name, providers[name], validate=True)
        if use_raw:
            print(json.dumps({"provider": name, "status": info["status"], "source": info["source"]}))
        else:
            source_part = f" ({info['source']})" if info["source"] else ""
            print(f"{name}: {info['status']}{source_part}")
        return 0

    if sub == "add":
        if len(tokens) < 8:
            print_error("provider add <name> --url URL --api_key KEY --default_model MODEL")
            return 1
        name = tokens[1]
        args = tokens[2:]
        params = {"url": None, "api_key": None, "default_model": None}
        i = 0
        while i < len(args):
            key = args[i]
            if key in {"--url", "--api_key", "--default_model"}:
                if i + 1 >= len(args):
                    print_error(f"{key} requires a value")
                    return 1
                params[key[2:]] = args[i + 1]
                i += 2
                continue
            print_error(f"Unknown option {key!r}")
            return 1
        if name in providers:
            print_error(f"Provider {name!r} already exists.")
            return 1
        if not all(params.values()):
            print_error("Missing required provider fields (--url --api_key --default_model).")
            return 1
        providers[name] = params
        if not save_providers_config(cfg):
            return 1
        # Save api key to file if provided
        if params.get("api_key"):
            save_api_key(name, params["api_key"])
        # initialize models file
        save_models(name, {"default_model": params["default_model"], "models": {}, "aliases": {}})
        print(f"Added provider {name}.")
        return 0

    if sub == "api-key":
        args = tokens[1:]
        api_key_usage = (
            "providers api-key set <name> [--value KEY]\n"
            "providers api-key show <name> [--reveal]\n"
            "providers api-key remove <name>"
        )
        if not args:
            print_error(f"Usage:\n{api_key_usage}")
            return 1

        action = args[0].lower()
        # Legacy compat: `providers api-key <name>` (no action keyword) treated as `set`
        if action not in {"set", "show", "remove"}:
            # Treat as provider name with implicit 'set'
            action = "set"
            args = ["set"] + args

        if action == "set":
            if len(args) < 2:
                print_error("providers api-key set <name> [--value KEY]")
                return 1
            name = args[1]
            if name not in providers:
                print_error(f"Provider {name!r} not found.")
                return 1
            # Parse optional --value
            key_value = None
            i = 2
            while i < len(args):
                if args[i] in {"--value", "-v"}:
                    if i + 1 >= len(args):
                        print_error("--value requires a value")
                        return 1
                    key_value = args[i + 1]
                    i += 2
                else:
                    print_error(f"Unknown option {args[i]!r}")
                    return 1
            if key_value is None:
                if sys.stdin.isatty():
                    key_value = getpass.getpass("Enter API key: ").strip()
                else:
                    key_value = sys.stdin.readline().strip()
            if not key_value:
                print_error("API key cannot be empty.")
                return 1
            if save_api_key(name, key_value):
                print(f"Saved API key for {name}.")
                return 0
            return 1

        if action == "show":
            if len(args) < 2:
                print_error("providers api-key show <name> [--reveal]")
                return 1
            name = args[1]
            if name not in providers:
                print_error(f"Provider {name!r} not found.")
                return 1
            reveal = any(a in {"--reveal", "-r"} for a in args[2:])
            key = load_api_key(name, providers[name])
            if not key:
                print(f"No API key configured for {name}.")
                return 0
            if reveal:
                print(key)
            else:
                if len(key) > 8:
                    print(f"{key[:4]}{'*' * (len(key) - 8)}{key[-4:]}")
                else:
                    print("*" * len(key))
            return 0

        if action == "remove":
            if len(args) < 2:
                print_error("providers api-key remove <name>")
                return 1
            name = args[1]
            if name not in providers:
                print_error(f"Provider {name!r} not found.")
                return 1
            if remove_api_key(name):
                print(f"Removed API key for {name}.")
            else:
                print(f"No API key file found for {name}.")
            return 0

        print_error(f"Usage:\n{api_key_usage}")
        return 1

    if sub == "rename":
        if len(tokens) != 3:
            print_error("provider rename <old> <new>")
            return 1
        old, new = tokens[1], tokens[2]
        if old not in providers:
            print_error(f"Provider {old!r} not found.")
            return 1
        if new in providers:
            print_error(f"Provider {new!r} already exists.")
            return 1
        providers[new] = providers.pop(old)
        save_providers_config(cfg)
        old_dir = PROVIDERS_ROOT / old
        new_dir = PROVIDERS_ROOT / new
        try:
            if old_dir.exists():
                old_dir.rename(new_dir)
        except OSError as exc:
            print_error(f"Warning: could not rename provider directory ({exc}).")
        print(f"Renamed provider {old} -> {new}.")
        return 0

    if sub in {"rm", "del", "delete"}:
        if len(tokens) != 2:
            print_error("provider rm <name>")
            return 1
        name = tokens[1]
        if name not in providers:
            print_error(f"Provider {name!r} not found.")
            return 1
        del providers[name]
        save_providers_config(cfg)
        target_dir = PROVIDERS_ROOT / name
        try:
            if target_dir.exists():
                for path in target_dir.glob("*"):
                    path.unlink(missing_ok=True)
                target_dir.rmdir()
        except OSError:
            pass
        print(f"Removed provider {name}.")
        return 0

    print_error(f"Usage:\n{usage}")
    return 1


def handle_models(rest: str | None, tokens_override: list[str] | None = None) -> int:
    usage = (
        "models list <provider> [--raw|-r] [--aliases|-a] [--table|-t] [--details|-d]\n"
        "models hosted <provider> [--raw|-r] [--table|-t]\n"
        "models default <provider> [model]\n"
        "models alias list <provider> [--raw|-r]\n"
        "models alias set <provider> <alias> <model>\n"
        "models alias rm <provider> <alias>\n"
        "models add <provider> <model> --name NAME [--description TEXT] [--max_tokens N] [--context_window N] [--default]\n"
        "models set <provider> <model> [--name NAME] [--description TEXT] [--max_tokens N] [--context_window N] [--default]\n"
        "models rm <provider> <model>"
    )
    tokens = tokens_override if tokens_override is not None else shlex.split(rest or "")
    if not tokens:
        print_error(f"Usage:\n{usage}")
        return 1
    sub = tokens[0].lower()
    if sub == "list":
        if len(tokens) < 2:
            print_error("models list <provider> [--raw|-r] [--aliases|-a] [--table|-t] [--details|-d]")
            return 1
        provider = tokens[1]
        raw = False
        use_table = False
        show_details = False
        include_aliases = False
        for token in tokens[2:]:
            if token in {"-r", "--raw"}:
                raw = True
            elif token in {"-a", "--aliases"}:
                include_aliases = True
            elif token in {"-t", "--table"}:
                use_table = True
            elif token in {"-d", "--details"}:
                show_details = True
            else:
                print_error(f"Unknown option for models list: {token!r}")
                return 1
        data = load_models(provider)
        aliases = load_model_aliases(provider)
        if raw:
            try:
                model_path = provider_models_path(provider)
                if not model_path.exists():
                    legacy_path = PROVIDERS_ROOT / provider / "models.yaml"
                    model_path = legacy_path if legacy_path.exists() else model_path
                raw_models = model_path.read_text() if model_path.exists() else ""
                printed = False
                if raw_models:
                    printed = print_pretty_json_if_possible(raw_models)
                if not printed and raw_models:
                    print(raw_models, end="" if raw_models.endswith("\n") else "\n")
                if include_aliases:
                    alias_path = provider_model_aliases_path(provider)
                    raw_aliases = alias_path.read_text() if alias_path.exists() else ""
                    if raw_aliases:
                        print("\n# aliases")
                        if not print_pretty_json_if_possible(raw_aliases):
                            print(raw_aliases, end="" if raw_aliases.endswith("\n") else "\n")
            except OSError as exc:
                print_error(f"Could not read models file: {exc}")
                return 1
        else:
            print_models(provider, data, aliases, table=use_table, details=show_details)
        return 0
    if sub == "hosted":
        if len(tokens) < 2:
            print_error("models hosted <provider> [--raw|-r] [--table|-t]")
            return 1
        provider = tokens[1]
        raw = False
        use_table = False
        for token in tokens[2:]:
            if token in {"-r", "--raw"}:
                raw = True
            elif token in {"-t", "--table"}:
                use_table = True
            else:
                print_error(f"Unknown option for model hosted: {token!r}")
                return 1
        provider_cfg_all = load_providers_config()
        providers = (
            provider_cfg_all.get("providers", {}) if isinstance(provider_cfg_all.get("providers", {}), dict) else {}
        )
        provider_cfg = providers.get(provider)
        if not provider_cfg:
            print_error(f"Provider {provider!r} not found in providers-config.yaml")
            return 1
        data = fetch_hosted_models(provider, provider_cfg)
        if data is None:
            return 1
        print_hosted_models(provider, provider_cfg, data, raw=raw, table=use_table)
        return 0

    if sub == "default":
        if len(tokens) not in {2, 3}:
            print_error("models default <provider> [model]")
            return 1
        provider = tokens[1]
        data = load_models(provider)
        models = data.get("models", {}) if isinstance(data.get("models", {}), dict) else {}
        if len(tokens) == 2:
            default_model = load_default_model(provider)
            if default_model:
                print(f"Default model for {provider}: {default_model}")
            else:
                print(f"No default model set for {provider}.")
            return 0
        new_default = tokens[2]
        if models and new_default not in models:
            print_error(f"Model {new_default!r} not found for provider {provider!r}.")
            return 1
        if not save_default_model(provider, new_default):
            print_error("Failed to save default model.")
            return 1
        print(f"Set default model for {provider} to {new_default}.")
        return 0

    if sub == "alias":
        alias_usage = (
            "models alias list <provider> [--raw|-r]\n"
            "models alias set <provider> <alias> <model>\n"
            "models alias rm <provider> <alias>"
        )
        if len(tokens) < 2:
            print_error(f"Usage:\n{alias_usage}")
            return 1
        alias_sub = tokens[1].lower()
        if alias_sub == "list":
            if len(tokens) < 3:
                print_error("models alias list <provider> [--raw|-r]")
                return 1
            provider = tokens[2]
            raw = any(t in {"-r", "--raw"} for t in tokens[3:])
            if raw:
                alias_path = provider_model_aliases_path(provider)
                if alias_path.exists():
                    content = alias_path.read_text()
                    if not print_pretty_json_if_possible(content):
                        print(content, end="" if content.endswith("\n") else "\n")
                else:
                    print(f"No aliases file for {provider}.")
            else:
                aliases = load_model_aliases(provider)
                if not aliases:
                    print(f"No aliases configured for {provider}.")
                else:
                    for alias_name, model_id in sorted(aliases.items()):
                        print(f"  {alias_name} -> {model_id}")
            return 0
        if alias_sub == "set":
            if len(tokens) != 5:
                print_error("models alias set <provider> <alias> <model>")
                return 1
            provider, alias_name, model_id = tokens[2], tokens[3], tokens[4]
            aliases = load_model_aliases(provider)
            aliases[alias_name] = model_id
            if not save_model_aliases(provider, aliases):
                print_error("Failed to save model aliases.")
                return 1
            print(f"Set alias '{alias_name}' -> '{model_id}' for {provider}.")
            return 0
        if alias_sub in {"rm", "del", "delete"}:
            if len(tokens) != 4:
                print_error("models alias rm <provider> <alias>")
                return 1
            provider, alias_name = tokens[2], tokens[3]
            aliases = load_model_aliases(provider)
            if alias_name not in aliases:
                print_error(f"Alias '{alias_name}' not found for {provider}.")
                return 1
            del aliases[alias_name]
            if not save_model_aliases(provider, aliases):
                print_error("Failed to save model aliases.")
                return 1
            print(f"Removed alias '{alias_name}' from {provider}.")
            return 0
        print_error(f"Usage:\n{alias_usage}")
        return 1

    if sub in {"add", "set"}:
        if len(tokens) < 3:
            print_error(f"Usage:\n{usage}")
            return 1
        provider, model = tokens[1], tokens[2]
        args = tokens[3:]
        meta: dict[str, object] = {}
        set_default = False
        i = 0
        while i < len(args):
            key = args[i]
            if key == "--name":
                if i + 1 >= len(args):
                    print_error("--name requires a value")
                    return 1
                meta["name"] = args[i + 1]
                i += 2
                continue
            if key == "--description":
                if i + 1 >= len(args):
                    print_error("--description requires a value")
                    return 1
                meta["description"] = args[i + 1]
                i += 2
                continue
            if key == "--max_tokens":
                if i + 1 >= len(args):
                    print_error("--max_tokens requires a value")
                    return 1
                try:
                    meta["max_tokens"] = int(args[i + 1])
                except ValueError:
                    print_error("--max_tokens must be an integer")
                    return 1
                i += 2
                continue
            if key == "--context_window":
                if i + 1 >= len(args):
                    print_error("--context_window requires a value")
                    return 1
                try:
                    meta["context_window"] = int(args[i + 1])
                except ValueError:
                    print_error("--context_window must be an integer")
                    return 1
                i += 2
                continue
            if key == "--default":
                set_default = True
                i += 1
                continue
            print_error(f"Unknown option {key!r}")
            return 1

        data = load_models(provider)
        models = data.setdefault("models", {})
        if not isinstance(models, dict):
            models = {}
            data["models"] = models
        existing = models.get(model) if isinstance(models.get(model), dict) else {}
        merged = {**existing, **meta}
        models[model] = merged
        if set_default:
            save_default_model(provider, model)
        if not save_models(provider, data):
            return 1
        action = "Updated" if sub == "set" and existing else "Added"
        print(f"{action} model {model} for {provider}.")
        return 0

    if sub in {"rm", "del", "delete"}:
        if len(tokens) != 3:
            print_error("model rm <provider> <model>")
            return 1
        provider, model = tokens[1], tokens[2]
        data = load_models(provider)
        models = data.get("models", {})
        if model not in models:
            print_error(f"Model {model!r} not found for provider {provider!r}.")
            return 1
        del models[model]
        default_model = load_default_model(provider)
        if default_model == model:
            new_default = next(iter(models.keys()), None)
            save_default_model(provider, new_default)
        save_models(provider, data)
        print(f"Removed model {model} from {provider}.")
        return 0

    print_error(f"Usage:\n{usage}")
    return 1


def handle_alias(rest: str | None, tokens_override: list[str] | None = None) -> int:
    usage = "alias list\nalias add <name> <expansion>\nalias rm <name>\nalias reset"
    tokens = tokens_override if tokens_override is not None else shlex.split(rest or "")
    if not tokens:
        tokens = ["list"]
    sub = tokens[0].lower()
    if sub in {"list", "ls"}:
        if not ALIASES:
            print("No aliases defined.")
            return 0
        names_to_show = set(tokens[1:]) if len(tokens) > 1 else None
        grouped: dict[str, list[str]] = {}
        for alias, cmd in ALIASES.items():
            grouped.setdefault(cmd, []).append(alias)
        for cmd in sorted(grouped):
            aliases_list = sorted(grouped[cmd])
            if names_to_show and cmd not in names_to_show and not any(a in names_to_show for a in aliases_list):
                continue
            aliases = ", ".join(aliases_list)
            print(f"{cmd:<12} - {aliases}")
        return 0
    if sub in {"add", "set"}:
        if len(tokens) < 3:
            print_error("alias add <name> <expansion>")
            return 1
        name = tokens[1]
        expansion = " ".join(tokens[2:]).strip()
        if not expansion:
            print_error("Expansion text is required.")
            return 1
        ALIASES[name] = expansion
        save_aliases()
        print(f"Alias saved: {name} -> {expansion}")
        return 0
    if sub in {"rm", "del", "delete"}:
        if len(tokens) != 2:
            print_error("alias rm <name>")
            return 1
        name = tokens[1]
        if name not in ALIASES:
            print_error(f"Alias {name!r} not found.")
            return 1
        del ALIASES[name]
        save_aliases()
        print(f"Removed alias {name}.")
        return 0
    if sub == "reset":
        reset_aliases_to_defaults()
        print("Aliases reset to defaults.")
        return 0
    print_error(f"Usage:\n{usage}")
    return 1


def command_aliases_for(target_command: str) -> list[str]:
    return sorted([alias for alias, cmd in ALIASES.items() if cmd == target_command])


def print_api_list(apis: dict[str, dict], *, raw: bool = False) -> None:
    """Print a formatted list of API definitions."""
    if raw:
        print(json.dumps({name: data for name, data in apis.items()}, indent=2))
        return
    if not apis:
        print("No API definitions found.")
        return
    if console and Table:
        table = Table(title="API Definitions")
        table.add_column("Name", style="cyan")
        table.add_column("Description")
        table.add_column("Path")
        table.add_column("Text Path")
        for name, data in apis.items():
            desc = data.get("description", "")
            path = data.get("request", {}).get("path", "") if isinstance(data.get("request"), dict) else ""
            resp = data.get("response", {}) if isinstance(data.get("response"), dict) else {}
            te = resp.get("text_extract", {}) if isinstance(resp.get("text_extract"), dict) else {}
            text_path = te.get("text_path") or (f"items:{te.get('items_path')}" if te.get("items_path") else "")
            table.add_row(name, desc, path, text_path)
        console.print(table)
    else:
        for name, data in apis.items():
            desc = data.get("description", "")
            print(f"  {name}: {desc}")


def print_api_definition(name: str, data: dict, *, raw: bool = False) -> None:
    """Print a single API definition with formatted output."""
    if raw:
        print(json.dumps(data, indent=2))
        return
    if console:
        console.print(f"[bold cyan]{name}[/bold cyan]")
        desc = data.get("description", "")
        if desc:
            console.print(f"  [dim]{desc}[/dim]")
        auth = data.get("auth", {})
        if auth:
            console.print(f"  [bold]Auth:[/bold] header={auth.get('header', 'Authorization')}, prefix={auth.get('value_prefix', 'Bearer ')!r}")
            extra = auth.get("extra_headers", {})
            if extra:
                for k, v in extra.items():
                    console.print(f"    {k}: {v}")
        req = data.get("request", {})
        if req:
            console.print(f"  [bold]Request:[/bold] {req.get('method', 'POST')} {req.get('path', '')}")
            schema = req.get("body_schema", {})
            props = schema.get("properties") if isinstance(schema, dict) else None
            if isinstance(props, dict) and props:
                console.print("  [bold]Body schema properties:[/bold]")
                required = set(schema.get("required", []))
                for pname, pdef in props.items():
                    if not isinstance(pdef, dict):
                        continue
                    ptype = pdef.get("type", "")
                    if isinstance(ptype, list):
                        ptype = "|".join(ptype)
                    enum = pdef.get("enum")
                    pdefault = pdef.get("default", "")
                    pdesc = pdef.get("description", "")
                    cli = pdef.get("x-cli", {})
                    flag = cli.get("flag") if isinstance(cli, dict) else None
                    line = f"    {pname} ({ptype})"
                    if pname in required:
                        line += " [required]"
                    if enum:
                        line += f" enum={enum}"
                    elif "minimum" in pdef or "maximum" in pdef:
                        lo = pdef.get("minimum", "-")
                        hi = pdef.get("maximum", "-")
                        line += f" range=[{lo}, {hi}]"
                    if pdefault != "":
                        line += f" default={pdefault}"
                    if flag:
                        line += f" cli={flag}"
                    if pdesc:
                        line += f" — {pdesc}"
                    console.print(line)
        presets = data.get("tool_presets")
        if isinstance(presets, dict) and presets:
            console.print("  [bold]Tool presets:[/bold] " + ", ".join(sorted(presets.keys())))
        resp = data.get("response", {})
        if resp:
            te = resp.get("text_extract", {}) if isinstance(resp.get("text_extract"), dict) else {}
            if te.get("text_path"):
                console.print(f"  [bold]Response:[/bold] text_path={te.get('text_path')}")
            elif te.get("items_path"):
                console.print(
                    f"  [bold]Response:[/bold] items_path={te.get('items_path')} "
                    f"item_filter={te.get('item_filter') or {}} "
                    f"content_path={te.get('content_path') or '(item)'}"
                )
            else:
                console.print("  [bold]Response:[/bold] (no text_extract configured)")
            meta = resp.get("metadata", {})
            if isinstance(meta, dict) and meta:
                console.print("  [bold]Metadata:[/bold]")
                for mk, mv in meta.items():
                    console.print(f"    {mk}: {mv}")
    else:
        print(f"{name}")
        desc = data.get("description", "")
        if desc:
            print(f"  {desc}")
        if yaml:
            print(yaml.safe_dump(data, sort_keys=False))
        else:
            print(json.dumps(data, indent=2))


def handle_api(rest: str | None, tokens_override: list[str] | None = None) -> int:
    """Handle the 'api' command and its subcommands."""
    usage = "api list [--raw|-r] | api show <name> [--raw|-r] | api create <name> [--from <existing>] [--path PATH] [--text-path PATH] | api edit <name> [--path PATH] [--text-path PATH] [--set-meta KEY:PATH] | api rm <name> | api validate <name> | api preview <name>|--file PATH [--prompt T] [--model M] [--role R]"
    tokens = tokens_override if tokens_override is not None else shlex.split(rest or "")
    if not tokens:
        print_error(f"Usage:\n{usage}")
        return 1

    sub = tokens[0]

    if sub == "list":
        raw = any(t in {"-r", "--raw"} for t in tokens[1:])
        apis = load_all_api_definitions()
        print_api_list(apis, raw=raw)
        return 0

    if sub == "show":
        if len(tokens) < 2:
            print_error("Usage: api show <name> [--raw|-r]")
            return 1
        name = tokens[1]
        raw = any(t in {"-r", "--raw"} for t in tokens[2:])
        data = load_api_definition(name)
        if data is None:
            print_error(f"API definition {name!r} not found.")
            return 1
        print_api_definition(name, data, raw=raw)
        return 0

    if sub == "create":
        if len(tokens) < 2:
            print_error("Usage: api create <name> [--from <existing>] [--path PATH] [--text-path PATH]")
            return 1
        name = tokens[1]
        if load_api_definition(name) is not None:
            print_error(f"API definition {name!r} already exists. Use 'api edit' to modify it.")
            return 1
        from_name: str | None = None
        path_val: str | None = None
        text_path_val: str | None = None
        idx = 2
        while idx < len(tokens):
            tok = tokens[idx]
            if tok == "--from":
                if idx + 1 >= len(tokens):
                    print_error("--from requires a value")
                    return 1
                from_name = tokens[idx + 1]
                idx += 2
            elif tok == "--path":
                if idx + 1 >= len(tokens):
                    print_error("--path requires a value")
                    return 1
                path_val = tokens[idx + 1]
                idx += 2
            elif tok == "--text-path":
                if idx + 1 >= len(tokens):
                    print_error("--text-path requires a value")
                    return 1
                text_path_val = tokens[idx + 1]
                idx += 2
            else:
                print_error(f"Unknown option {tok!r}")
                return 1
        if from_name:
            source = load_api_definition(from_name)
            if source is None:
                print_error(f"Source API definition {from_name!r} not found.")
                return 1
            import copy
            data = copy.deepcopy(source)
        else:
            data = {
                "name": name,
                "description": "",
                "auth": {"header": "Authorization", "value_prefix": "Bearer "},
                "request": {
                    "method": "POST",
                    "path": path_val or "/chat/completions",
                    "body_schema": {
                        "type": "object",
                        "required": ["model", "messages"],
                        "properties": {
                            "model": {"type": "string", "x-bind": {"from": "model"}},
                            "messages": {
                                "type": "array",
                                "x-render": {
                                    "template": [
                                        {"role": "{{role}}", "content": "{{prompt}}"}
                                    ]
                                },
                            },
                            "temperature": {
                                "type": "number",
                                "minimum": 0,
                                "maximum": 2,
                                "x-cli": {"flag": "--temperature", "alias": "-t"},
                            },
                            "max_tokens": {
                                "type": "integer",
                                "minimum": 1,
                                "x-cli": {"flag": "--max-tokens", "alias": "-m"},
                            },
                        },
                    },
                },
                "response": {
                    "text_extract": {
                        "text_path": text_path_val or "choices[0].message.content"
                    }
                },
            }
        data["name"] = name
        if path_val:
            data.setdefault("request", {})["path"] = path_val
        if text_path_val:
            resp = data.setdefault("response", {})
            te = resp.setdefault("text_extract", {})
            te["text_path"] = text_path_val
            te.pop("items_path", None)
            te.pop("item_filter", None)
            te.pop("content_path", None)
            te.pop("content_filter", None)
        if save_api_definition(name, data):
            print(f"API definition {name!r} created.")
            return 0
        return 1

    if sub == "edit":
        if len(tokens) < 2:
            print_error("Usage: api edit <name> [--path PATH] [--text-path PATH] [--set-meta KEY:PATH]")
            return 1
        name = tokens[1]
        data = load_api_definition(name)
        if data is None:
            print_error(f"API definition {name!r} not found.")
            return 1
        idx = 2
        while idx < len(tokens):
            tok = tokens[idx]
            if tok == "--path":
                if idx + 1 >= len(tokens):
                    print_error("--path requires a value")
                    return 1
                data.setdefault("request", {})["path"] = tokens[idx + 1]
                idx += 2
            elif tok == "--text-path":
                if idx + 1 >= len(tokens):
                    print_error("--text-path requires a value")
                    return 1
                resp = data.setdefault("response", {})
                te = resp.setdefault("text_extract", {})
                te["text_path"] = tokens[idx + 1]
                # Switching to simple mode: clear walked-mode keys.
                te.pop("items_path", None)
                te.pop("item_filter", None)
                te.pop("content_path", None)
                te.pop("content_filter", None)
                idx += 2
            elif tok == "--set-meta":
                if idx + 1 >= len(tokens):
                    print_error("--set-meta requires a value (KEY:PATH)")
                    return 1
                parts = tokens[idx + 1].split(":", 1)
                if len(parts) != 2:
                    print_error("--set-meta format: KEY:PATH")
                    return 1
                resp = data.setdefault("response", {})
                meta = resp.setdefault("metadata", {})
                meta[parts[0]] = parts[1]
                idx += 2
            else:
                print_error(f"Unknown option {tok!r}")
                return 1
        if save_api_definition(name, data):
            print(f"API definition {name!r} updated.")
            return 0
        return 1

    if sub == "rm":
        if len(tokens) < 2:
            print_error("Usage: api rm <name>")
            return 1
        name = tokens[1]
        if delete_api_definition(name):
            print(f"API definition {name!r} removed.")
            return 0
        print_error(f"API definition {name!r} not found.")
        return 1

    if sub == "validate":
        if len(tokens) < 2:
            print_error("Usage: api validate <name>")
            return 1
        name = tokens[1]
        data = load_api_definition(name)
        if data is None:
            print_error(f"API definition {name!r} not found.")
            return 1
        errors = validate_api_definition(data)
        if errors:
            print_error(f"Validation errors for {name!r}:")
            for err in errors:
                print_error(f"  - {err}")
            return 1
        print(f"API definition {name!r} is valid.")
        return 0

    if sub == "preview":
        # Usage: api preview <name> [--prompt T] [--model M] [--role R]
        #        api preview --file PATH [--prompt T] [--model M] [--role R]
        from_file: str | None = None
        target_name: str | None = None
        sample_prompt = "Sample prompt"
        sample_role = "user"
        sample_model = "sample-model"
        idx = 1
        while idx < len(tokens):
            tok = tokens[idx]
            if tok == "--file":
                if idx + 1 >= len(tokens):
                    print_error("--file requires a path")
                    return 1
                from_file = tokens[idx + 1]
                idx += 2
                continue
            if tok == "--prompt":
                if idx + 1 >= len(tokens):
                    print_error("--prompt requires a value")
                    return 1
                sample_prompt = tokens[idx + 1]
                idx += 2
                continue
            if tok == "--model":
                if idx + 1 >= len(tokens):
                    print_error("--model requires a value")
                    return 1
                sample_model = tokens[idx + 1]
                idx += 2
                continue
            if tok == "--role":
                if idx + 1 >= len(tokens):
                    print_error("--role requires a value")
                    return 1
                sample_role = tokens[idx + 1]
                idx += 2
                continue
            if not tok.startswith("-") and target_name is None:
                target_name = tok
                idx += 1
                continue
            print_error(f"Unknown option {tok!r}")
            return 1

        if from_file:
            try:
                with open(from_file) as fh:
                    text = fh.read()
            except OSError as exc:
                print_error(f"Could not read file: {exc}")
                return 1
            if not yaml:
                print(json.dumps({
                    "valid": False,
                    "errors": ["PyYAML not available; install pyyaml to use --file"],
                }))
                return 1
            try:
                data = yaml.safe_load(text)
            except yaml.YAMLError as exc:
                print(json.dumps({"valid": False, "errors": [f"YAML parse error: {exc}"]}))
                return 0
            if not isinstance(data, dict):
                print(json.dumps({"valid": False, "errors": ["YAML top level must be an object"]}))
                return 0
        else:
            if not target_name:
                print_error("api preview <name> | api preview --file PATH")
                return 1
            data = load_api_definition(target_name)
            if data is None:
                print_error(f"API definition {target_name!r} not found.")
                return 1

        errors = validate_api_definition(data)
        result: dict[str, object] = {
            "name": data.get("name", target_name or ""),
            "description": data.get("description", ""),
            "valid": not errors,
            "errors": errors,
            "flags": api_compose.cli_flags_for(data),
            "presets": api_compose.tool_preset_names(data),
        }
        if not errors:
            try:
                body = lexi.compose_request_body(
                    data,
                    prompt_payload={"prompt": sample_prompt, "role": sample_role},
                    overrides={"model": sample_model},
                )
                result["body"] = body
            except (ValueError, KeyError) as exc:
                result["body_error"] = str(exc)
        print(json.dumps(result, indent=2, default=str))
        return 0

    print_error(f"Unknown api subcommand {sub!r}.\nUsage:\n{usage}")
    return 1


def handle_respond(rest: str | None, tokens_override: list[str] | None = None) -> int:
    usage = (
        "respond <provider> [prompt_name] [--model MODEL] [<schema flags>] "
        "[--set path=value] [--unset path] [--tool preset[,preset]] "
        "[--params file] [--cite] [--raw|-r] [--verbose|-v]"
    )
    tokens = tokens_override if tokens_override is not None else shlex.split(rest or "")
    if not tokens:
        print_error(f"Usage:\n{usage}")
        return 1
    provider = tokens[0]
    prompt_name = PROMPT_ACTIVE_NAME
    flag_start = 1
    if len(tokens) > 1 and not tokens[1].startswith("-"):
        prompt_name = tokens[1]
        flag_start = 2

    provider_cfg_all = load_providers_config()
    providers = provider_cfg_all.get("providers", {}) if isinstance(provider_cfg_all.get("providers", {}), dict) else {}
    provider_cfg = providers.get(provider)
    if not provider_cfg:
        print_error(f"Provider {provider!r} not found in providers-config.yaml")
        return 1

    # Resolve effective config so we know the schema before parsing flags.
    effective_cfg_for_flags = resolve_effective_config(provider, provider_cfg)
    flag_specs = api_compose.cli_flags_for(effective_cfg_for_flags)
    presets_available = api_compose.tool_preset_names(effective_cfg_for_flags)

    parsed, err = _parse_respond_flags(flag_specs, presets_available, tokens, start=flag_start)
    if err:
        print_error(err)
        return 1

    model_override = parsed["model_override"]
    if model_override:
        aliases = load_model_aliases(provider)
        if model_override in aliases:
            model_override = aliases[model_override]
        else:
            models_data = load_models(provider)
            configured = set()
            if isinstance(models_data, dict):
                models_section = models_data.get("models", models_data)
                if isinstance(models_section, dict):
                    configured = set(models_section.keys())
            if configured and model_override not in configured:
                alias_names = sorted(aliases.keys()) if aliases else []
                msg = f"Model {model_override!r} is not a configured model or alias for provider {provider!r}."
                if alias_names:
                    msg += f"\nAvailable aliases: {', '.join(alias_names)}"
                if configured:
                    msg += f"\nConfigured models: {', '.join(sorted(configured))}"
                print_error(msg)
                return 1

    if not model_override:
        file_default = load_default_model(provider)
        if file_default:
            provider_cfg = {**provider_cfg, "default_model": file_default}

    effective_model = model_override or provider_cfg.get("default_model")
    if effective_model:
        models_data = load_models(provider)
        models_section = models_data.get("models", models_data) if isinstance(models_data, dict) else {}
        model_meta = models_section.get(effective_model, {}) if isinstance(models_section, dict) else {}
        if isinstance(model_meta, dict):
            for key in ("auth", "request", "response"):
                if key in model_meta:
                    provider_cfg = dict(provider_cfg)
                    existing = provider_cfg.get(key)
                    if isinstance(existing, dict) and isinstance(model_meta[key], dict):
                        provider_cfg[key] = deep_merge(existing, model_meta[key])
                    else:
                        provider_cfg[key] = model_meta[key]

    prompt_payload = extract_prompt_payload(prompt_name)
    if prompt_payload is None:
        print_error(f"Prompt {prompt_name!r} not found.")
        return 1

    effective_cfg = resolve_effective_config(provider, provider_cfg)

    overrides: dict[str, object] = {}
    if parsed["params_file"]:
        params_data, perr = _load_params_file(parsed["params_file"])
        if perr:
            print_error(perr)
            return 1
        overrides.update(params_data)

    code, resp_json, was_streamed = respond_with_provider(
        provider,
        effective_cfg,
        prompt_payload,
        model_override=model_override,
        overrides=overrides,
        set_paths=parsed["set_paths"],
        unset_paths=parsed["unset_paths"],
        tool_presets_selected=parsed["tool_presets_selected"] or None,
        raw_mode=parsed["raw"],
    )
    if code != 0:
        return code
    if was_streamed:
        # Incremental text was already streamed to stdout (or suppressed for raw).
        if parsed["raw"] and resp_json is not None:
            render_response_output(resp_json, raw=True, provider_cfg=effective_cfg)
        elif parsed["cite"] and resp_json is not None:
            # Streaming printed the answer; we still want sources at the end.
            render_response_output(
                resp_json,
                raw=False,
                provider_cfg=effective_cfg,
                cite=True,
                print_text=False,
            )
    else:
        render_response_output(
            resp_json,
            raw=parsed["raw"],
            provider_cfg=effective_cfg,
            cite=parsed["cite"],
        )
    if parsed["verbose"] and not parsed["raw"] and resp_json:
        metadata = extract_response_metadata(resp_json, effective_cfg)
        activity_cfg = (effective_cfg.get("response") or {}).get("activity")
        activity_summary = lexi.summarize_activity(resp_json, activity_cfg)
        print_response_metadata(metadata, activity_summary)
    return 0


def execute_cli_line(line: str, *, record: bool = True, alias_depth: int = 0) -> tuple[int, bool]:
    """Process a single CLI line. Returns (exit_code, should_exit)."""
    stripped = line.strip()
    if not stripped:
        return 0, False

    try:
        parts = shlex.split(stripped)
    except ValueError as exc:
        print_error(f"Could not parse command ({exc}). For multi-line prompts, escape newlines as \\n.")
        return 1, False

    if not parts:
        return 0, False

    if parts[0].startswith("#"):
        ref = stripped[1:].strip()
        if ref == "-":
            index = -1
        elif not ref:
            print_error("Usage: #<number> or # <number>")
            return 1, False
        else:
            try:
                index = int(ref)
            except ValueError:
                print_error("history reference must be an integer (e.g., #3).")
                return 1, False
        return run_history_entry(index)

    if parts[0].startswith("!"):
        rest = stripped[1:].strip()
        if record:
            record_history(stripped)
        return handle_sh(rest), False

    command, *args = parts
    rest = " ".join(args) if args else None

    if command in ALIASES:
        if record:
            record_history(stripped)
        if alias_depth > 5:
            print_error("Alias expansion too deep; possible recursion.")
            return 1, False
        expanded = ALIASES[command]
        if rest:
            expanded = f"{expanded} {rest}"
        return execute_cli_line(expanded, record=False, alias_depth=alias_depth + 1)

    if command.lower() in EXIT_COMMANDS and not rest:
        if record:
            record_history(command)
        return 0, True

    if record:
        record_history(stripped)

    code = dispatch(command, rest, args)
    return code, False


def print_help(target: str | None = None) -> None:
    if target:
        tokens = shlex.split(target)
        lookup = tokens[0].lower() if tokens else ""
        sub_lookup = tokens[1].lower() if len(tokens) > 1 else None
        # Resolve command aliases
        if lookup in ALIASES:
            lookup = ALIASES[lookup]
        if lookup in {"general", "commands"}:
            print("General Commands:")
            for name, desc in GENERAL_COMMANDS.items():
                print_command_with_subcommands(name, desc)
            return
        if lookup in {"lexi", "lexi-commands", "ai", "ai-commands"}:
            print("Lexi Commands:")
            for name, desc in LEXI_COMMANDS.items():
                print_command_with_subcommands(name, desc)
            return
        if lookup in {"shortcuts", "shortcut"}:
            print("Shortcuts:")
            for name, desc in SHORTCUTS.items():
                print(f"  {name:<12} - {desc}")
            return

        if lookup == "!":
            print("! - Shortcut for 'sh' in interactive mode.")
            return
        if lookup == "?":
            print("? - Shortcut for 'help'.")
            return
        desc = GENERAL_COMMANDS.get(lookup) or LEXI_COMMANDS.get(lookup)
        if desc:
            if sub_lookup and lookup in COMMAND_SUBCOMMANDS:
                subcommands = COMMAND_SUBCOMMANDS[lookup]
                sub_meta = subcommands.get(sub_lookup)
                if sub_meta:
                    usage = sub_meta.get("usage")
                    summary = sub_meta.get("summary", "")
                    print(f"{lookup} {sub_lookup} - {summary}")
                    if usage:
                        print(f"  usage: {usage}")
                    return
            print_command_with_subcommands(lookup, desc)
            aliases_for_cmd = command_aliases_for(lookup)
            if aliases_for_cmd:
                print(f"  aliases: {', '.join(aliases_for_cmd)}")
            return

        shortcut_key = target if target in SHORTCUTS else target.upper() if target in {"↑", "↓"} else None
        if shortcut_key and shortcut_key in SHORTCUTS:
            print(f"{shortcut_key} - {SHORTCUTS[shortcut_key]}")
            return

        if target in {"#", "#<N>"}:
            print(f"#<N> - {SHORTCUTS['#<N>']}")
            return

        print_error(f"No help available for {target!r}.")
        return

    print("Lexi Commands:")
    for name, desc in LEXI_COMMANDS.items():
        print(f"  {name:<12} - {desc}")

    print("\nGeneral Commands:")
    for name, desc in GENERAL_COMMANDS.items():
        print(f"  {name:<12} - {desc}")

    print("\nShortcuts:")
    for name, desc in SHORTCUTS.items():
        print(f"  {name:<12} - {desc}")


def handle_sh(command: str | None) -> int:
    if not command:
        print_error("Usage: sh <command>")
        return 1
    return run_command(command)


def print_version() -> int:
    name = CONFIG.get("cli_name", CONFIG_DEFAULT["cli_name"])
    version = get_cli_version()
    print(f"{name}:{version}")
    return 0


def dispatch(command: str, rest: str | None, args: list[str] | None = None) -> int:
    cmd = command.lower()
    if cmd == "help":
        print_help(rest)
        return 0
    if cmd == "?":
        print_help(rest)
        return 0
    if cmd in EXIT_COMMANDS:
        return 0
    if cmd == "sh":
        return handle_sh(rest)
    if cmd == "history":
        return handle_history(rest)
    if cmd == "config":
        return handle_config(rest, tokens_override=args)
    if cmd == "prompts":
        return handle_prompt(rest, tokens_override=args)
    if cmd == "providers":
        return handle_provider(rest, tokens_override=args)
    if cmd == "models":
        return handle_models(rest, tokens_override=args)
    if cmd == "alias":
        return handle_alias(rest, tokens_override=args)
    if cmd == "api":
        return handle_api(rest, tokens_override=args)
    if cmd == "respond":
        return handle_respond(rest, tokens_override=args)
    if cmd == "version":
        return print_version()

    print_error(f"Unknown command: {command!r}. Type 'help' to list commands.")
    return 1


def interactive_loop() -> int:
    """Read commands until EOF or an explicit exit command."""
    try:
        while True:
            try:
                prompt = CONFIG.get("prompt", CONFIG_DEFAULT["prompt"])
                delim = CONFIG.get("prompt_delimiter", CONFIG_DEFAULT["prompt_delimiter"])
                line = input(f"{prompt}{delim} ").strip()
            except EOFError:
                print()
                return 0

            code, should_exit = execute_cli_line(line)
            if should_exit:
                return 0

    except KeyboardInterrupt:
        print()
        return 130

    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run a single CLI command or enter an interactive shell loop.")
    parser.add_argument(
        "command",
        nargs=argparse.REMAINDER,
        help="CLI command to run once; leave empty to enter interactive mode.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    argv = argv if argv is not None else sys.argv[1:]
    ensure_data_dir()
    load_config()
    load_history()
    load_prompts()
    load_aliases()
    enable_line_editing()

    args = parse_args(argv)
    if args.command:
        command, *rest_tokens = args.command
        rest = " ".join(rest_tokens) if rest_tokens else None
        code = dispatch(command, rest, rest_tokens)
        return code
    return interactive_loop()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
