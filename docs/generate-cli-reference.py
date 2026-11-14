#!/usr/bin/env python3
"""Generate docs/cli-reference.md from the command metadata in cli.py.

Usage:
    python docs/generate-cli-reference.py > docs/cli-reference.md

Or run from the repo root:
    python docs/generate-cli-reference.py
"""

import sys
from pathlib import Path

# Add the CLI source to the path so we can import the metadata dicts
CLI_SRC = Path(__file__).resolve().parent.parent / "lexi-cli" / "src"
sys.path.insert(0, str(CLI_SRC))

from lexi_cli.cli import LEXI_COMMANDS, COMMAND_SUBCOMMANDS, SHORTCUTS  # noqa: E402


def generate():
    lines: list[str] = []
    w = lines.append

    w("# CLI Reference")
    w("")
    w("Complete command reference for the Lexi CLI.")
    w("This file is auto-generated — run `python docs/generate-cli-reference.py` to update.")
    w("")
    w("---")
    w("")

    # Extra usage info for commands without subcommands
    STANDALONE_USAGE = {
        "respond": {
            "usage": "respond <provider> [prompt_name] [--model M] [--temperature T] [--max_tokens N] [--web-search] [--raw|-r]",
            "notes": [
                "Sends the active prompt (or a named prompt) to the specified provider.",
                "Uses the provider's default model unless `--model` is given.",
                "Pass `--raw` to print the full JSON response instead of extracted text.",
            ],
        },
    }

    # --- Lexi commands ---
    w("## Commands")
    w("")
    for cmd, desc in LEXI_COMMANDS.items():
        w(f"### `{cmd}`")
        w("")
        w(desc)
        w("")
        subs = COMMAND_SUBCOMMANDS.get(cmd, {})
        if subs:
            for sub, info in subs.items():
                summary = info.get("summary", "")
                usage = info.get("usage", f"{cmd} {sub}")
                w(f"**`{cmd} {sub}`** — {summary}")
                w("")
                w(f"```")
                w(usage)
                w(f"```")
                w("")
        elif cmd in STANDALONE_USAGE:
            extra = STANDALONE_USAGE[cmd]
            w(f"```")
            w(extra["usage"])
            w(f"```")
            w("")
            for note in extra.get("notes", []):
                w(f"- {note}")
            w("")
        w("---")
        w("")

    # --- General commands (config, alias, help, etc.) ---
    w("## General Commands")
    w("")
    general = ["config", "alias"]
    for cmd in general:
        subs = COMMAND_SUBCOMMANDS.get(cmd, {})
        if not subs:
            continue
        w(f"### `{cmd}`")
        w("")
        for sub, info in subs.items():
            summary = info.get("summary", "")
            usage = info.get("usage", f"{cmd} {sub}")
            w(f"**`{cmd} {sub}`** — {summary}")
            w("")
            w(f"```")
            w(usage)
            w(f"```")
            w("")
        w("---")
        w("")

    w("### `help`")
    w("")
    w("Show help for commands and topics.")
    w("")
    w("```")
    w("help [topic|command [subcommand]]")
    w("```")
    w("")
    w("Topics: `ai`, `general`, `shortcuts`")
    w("")
    w("---")
    w("")

    w("### `history`")
    w("")
    w("Show or clear command history.")
    w("")
    w("```")
    w("history [-n COUNT] [--reset|--clear]")
    w("```")
    w("")
    w("---")
    w("")

    w("### `sh`")
    w("")
    w("Run a shell command.")
    w("")
    w("```")
    w("sh <command>")
    w("!<command>")
    w("```")
    w("")
    w("---")
    w("")

    w("### `version`")
    w("")
    w("Show the CLI version.")
    w("")
    w("```")
    w("version")
    w("```")
    w("")
    w("---")
    w("")

    w("### `exit`")
    w("")
    w("Exit the CLI.")
    w("")
    w("```")
    w("exit | quit | e | q")
    w("```")
    w("")
    w("---")
    w("")

    # --- Shortcuts ---
    w("## Shortcuts")
    w("")
    w("| Shortcut | Description |")
    w("|---|---|")
    for shortcut, desc in SHORTCUTS.items():
        w(f"| `{shortcut}` | {desc} |")
    w("")

    return "\n".join(lines)


if __name__ == "__main__":
    output = generate()
    out_path = Path(__file__).resolve().parent / "cli-reference.md"
    out_path.write_text(output + "\n")
    print(f"Wrote {out_path}")
