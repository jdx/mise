#!/usr/bin/env python3
import sys
import tomllib

def main():
    try:
        with open("registry.toml", "rb") as f:
            data = tomllib.load(f)
    except FileNotFoundError:
        print("Error: registry.toml not found in the current directory.", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error parsing registry.toml: {e}", file=sys.stderr)
        sys.exit(1)

    tools = data.get("tools", {})
    missing_tests = []

    for tool_name, tool_config in tools.items():
        if "test" not in tool_config:
            missing_tests.append(tool_name)

    failed_tools = set()
    try:
        with open("failed_tools.txt", "r") as f:
            for line in f:
                failed_tools.add(line.strip())
    except FileNotFoundError:
        pass

    for tool in sorted(missing_tests):
        failed_mark = " (failed)" if tool in failed_tools else ""
        print(f"{tool}{failed_mark}")

if __name__ == "__main__":
    main()
