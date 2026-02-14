#!/usr/bin/env python3
"""Remove [key=value] format from registry TOML backend strings.

Converts inline bracket options to expanded TOML table format.
For files with backends = [...] arrays that contain bracketed entries,
ALL backends are converted to [[backends]] table entries and moved to the END of the file
to preserve TOML structure (root keys must come before arrays of tables).
"""
#MISE description="Remove [key=value] bracket options from registry backend strings"

import re
import sys
import tomllib
from pathlib import Path

REGISTRY_DIR = Path(__file__).resolve().parent.parent / "registry"

# Pattern to match bracket options: [key=val,key2=val2]
BRACKET_OPTS_RE = re.compile(r'\[([a-z_]+=.*?)\]')


def parse_bracket_opts(opts_str: str) -> dict[str, str]:
    """Parse 'key=val,key2=val2' into a dict."""
    result = {}
    for pair in opts_str.split(","):
        key, _, value = pair.partition("=")
        result[key.strip()] = value.strip()
    return result


def strip_bracket_opts(s: str) -> tuple[str, dict[str, str] | None]:
    """Strip [key=value,...] from a backend string, returning (cleaned, opts)."""
    m = BRACKET_OPTS_RE.search(s)
    if not m:
        return s, None
    opts = parse_bracket_opts(m.group(1))
    cleaned = s[:m.start()] + s[m.end():]
    return cleaned, opts


def format_opts_value(v: str) -> str:
    """Format an option value for TOML output."""
    if v in ("true", "false"):
        return v
    return f'"{v}"'


def process_file(filepath: Path) -> bool:
    """Process a single registry TOML file. Returns True if modified."""
    content = filepath.read_text()

    # Quick check: does this file contain bracket opts in backend strings?
    if not BRACKET_OPTS_RE.search(content):
        return False

    # Parse with tomllib to understand structure
    try:
        data = tomllib.loads(content)
    except Exception as e:
        print(f"  ERROR parsing {filepath.name}: {e}", file=sys.stderr)
        return False

    backends = data.get("backends", [])

    # Determine which format the file uses
    # Case A: backends is an inline array (list of strings/dicts)
    # Case B: backends uses [[backends]] table format

    has_inline_array = bool(re.search(r'^backends\s*=\s*\[', content, re.MULTILINE))
    has_table_array = bool(re.search(r'^\[\[backends\]\]', content, re.MULTILINE))

    if has_inline_array and not has_table_array:
        return process_inline_array(filepath, content, data)
    elif has_table_array:
        return process_table_array(filepath, content)
    else:
        # Fallback or weird mix
        return False


def process_inline_array(filepath: Path, content: str, data: dict) -> bool:
    """Handle files where backends is defined as an inline array.
    Moves backends to the bottom as [[backends]] tables.
    """
    backends = data.get("backends", [])

    lines = content.split("\n")

    # Find the backends = [...] block (may span multiple lines)
    backends_start = None
    backends_end = None
    bracket_depth = 0
    in_backends = False
    for i, line in enumerate(lines):
        if not in_backends and re.match(r'^backends\s*=\s*\[', line):
            backends_start = i
            in_backends = True
            bracket_depth = line.count("[") - line.count("]")
            if bracket_depth == 0:
                backends_end = i
                break
        elif in_backends:
            bracket_depth += line.count("[") - line.count("]")
            if bracket_depth <= 0:
                backends_end = i
                break

    if backends_start is None or backends_end is None:
        return False

    # Collect content other than backends definition
    before_lines = lines[:backends_start]
    after_lines = lines[backends_end + 1:]
    
    # Combine remaining lines and clean up empty ones
    other_lines = before_lines + after_lines
    
    # Trim leading/trailing empty lines from the "other" block
    # But preserve structure. Just trim trailing.
    while other_lines and other_lines[-1].strip() == "":
        other_lines.pop()
        
    expanded_entries = []
    for entry in backends:
        if isinstance(entry, str):
            cleaned, opts = strip_bracket_opts(entry)
            expanded_entries.append({"full": cleaned, "opts": opts})
        elif isinstance(entry, dict):
            full = entry.get("full", "")
            cleaned, opts = strip_bracket_opts(full)
            extra = {k: v for k, v in entry.items() if k != "full"}
            expanded_entries.append({"full": cleaned, "opts": opts, "extra": extra})

    # Build new content
    result_lines = other_lines[:] # Root keys first
    
    # Add expanded backends at the end
    for entry in expanded_entries:
        result_lines.append("")
        result_lines.append("[[backends]]")
        full_val = entry["full"]
        extra = entry.get("extra", {})
        opts = entry.get("opts")

        if extra and "platforms" in extra:
            platforms = extra["platforms"]
            platform_strs = ", ".join(f'"{p}"' for p in platforms)
            result_lines.append(f'full = "{full_val}"')
            result_lines.append(f'platforms = [{platform_strs}]')
        else:
            result_lines.append(f'full = "{full_val}"')
            for k, v in extra.items():
                if isinstance(v, list):
                    list_strs = ", ".join(f'"{x}"' for x in v)
                    result_lines.append(f'{k} = [{list_strs}]')
                elif isinstance(v, str):
                    result_lines.append(f'{k} = "{v}"')
                elif isinstance(v, bool):
                    result_lines.append(f'{k} = {"true" if v else "false"}')

        if opts:
            result_lines.append("")
            result_lines.append("[backends.options]")
            for k, v in opts.items():
                result_lines.append(f'{k} = {format_opts_value(v)}')

    # Ensure ends with newline
    result = "\n".join(result_lines).rstrip("\n") + "\n"

    # Clean up double blank lines
    while "\n\n\n" in result:
        result = result.replace("\n\n\n", "\n\n")
    
    # Ensure simplified single newline at START if original file started with empty line
    # (Not critical, but good for polish)
    result = result.lstrip("\n")

    if result != content:
        filepath.write_text(result)
        return True
    return False


def process_table_array(filepath: Path, content: str) -> bool:
    """Handle files where backends already uses [[backends]] table format.

    Just strip [key=value] from full = "..." lines and add options.
    """
    lines = content.split("\n")
    new_lines = []
    modified = False
    i = 0

    while i < len(lines):
        line = lines[i]

        full_match = re.match(r'^(\s*)full\s*=\s*"(.*)"(.*)$', line)
        if full_match:
            indent = full_match.group(1)
            full_val = full_match.group(2)
            trailing = full_match.group(3)
            cleaned, opts = strip_bracket_opts(full_val)
            if opts:
                modified = True
                new_lines.append(f'{indent}full = "{cleaned}"{trailing}')

                j = i + 1
                while j < len(lines) and lines[j].strip() == "":
                    j += 1

                has_existing_options = (
                    j < len(lines) and
                    lines[j].strip() == "[backends.options]"
                )
                
                has_sub_table = (
                    j < len(lines) and
                    lines[j].strip().startswith("[backends.options.")
                )

                if has_existing_options:
                    # Append strictly to existing [backends.options] block
                    # Skip writing a new header
                    # Pass through lines until we hit the header, then write our keys after it
                    
                    # But we are in a loop...
                    # Strategy: Write blank line, then consume lines until we print the header,
                    # then print our keys.
                    new_lines.append("")
                    for k in range(i + 1, j + 1): # include j (the header)
                         if lines[k].strip() != "":
                             new_lines.append(lines[k])
                    
                    # Insert new keys
                    for k, v in opts.items():
                        new_lines.append(f'{k} = {format_opts_value(v)}')
                    
                    i = j + 1 
                    continue

                elif has_sub_table:
                   # [backends.options] parent table not defined, only sub-tables.
                   # We can define [backends.options] safely.
                    new_lines.append("")
                    new_lines.append("[backends.options]")
                    for k, v in opts.items():
                        new_lines.append(f'{k} = {format_opts_value(v)}')
                    i += 1
                    continue
                else:
                    # No options section at all. Create one.
                    new_lines.append("")
                    new_lines.append("[backends.options]")
                    for k, v in opts.items():
                        new_lines.append(f'{k} = {format_opts_value(v)}')
                    i += 1
                    continue
            else:
                new_lines.append(line)
                i += 1
                continue

        # Check for inline table { full = "...[opts]...", ... }
        inline_match = re.search(r'\{\s*full\s*=\s*"([^"]*\[[a-z_]+=.*?\][^"]*)"', line)
        if inline_match:
            full_val = inline_match.group(1)
            cleaned, opts = strip_bracket_opts(full_val)
            if opts:
                modified = True
                old_full = f'"{full_val}"'
                new_full = f'"{cleaned}"'
                modified_line = line.replace(old_full, new_full)

                # Build options inline table
                opts_parts = []
                for k, v in opts.items():
                    opts_parts.append(f'{k} = {format_opts_value(v)}')
                opts_inline = "{ " + ", ".join(opts_parts) + " }"

                # Insert options before closing }
                modified_line = re.sub(
                    r'\}\s*(,?)(\s*)$',
                    f', options = {opts_inline} }}\\1\\2',
                    modified_line
                )
                new_lines.append(modified_line)
                i += 1
                continue

        new_lines.append(line)
        i += 1

    if not modified:
        return False

    result = "\n".join(new_lines)
    while "\n\n\n" in result:
        result = result.replace("\n\n\n", "\n\n")
    result = result.rstrip("\n") + "\n"

    filepath.write_text(result)
    return True


def main():
    if not REGISTRY_DIR.is_dir():
        print(f"Registry directory not found: {REGISTRY_DIR}", file=sys.stderr)
        sys.exit(1)

    files = sorted(REGISTRY_DIR.glob("*.toml"))
    modified = 0
    for f in files:
        if process_file(f):
            print(f"  Modified: {f.name}")
            modified += 1

    print(f"\nDone. Modified {modified} files.")


if __name__ == "__main__":
    main()
