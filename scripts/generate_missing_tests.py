#!/usr/bin/env python3
import sys
import subprocess
import re
import tomllib
import os

def get_registry_content():
    try:
        with open("registry.toml", "r") as f:
            return f.read()
    except FileNotFoundError:
        print("Error: registry.toml not found.", file=sys.stderr)
        sys.exit(1)

def parse_toml(content):
    return tomllib.loads(content)

def is_test_commented_out(tool_name, content):
    # This is a heuristic. It looks for the tool block and checks for a commented test line.
    # It's not perfect but should catch most cases.
    tool_header = f"[tools.{tool_name}]"
    
    # regex to find the block for the tool
    # It matches from [tools.name] until the next [ (start of next section) or EOF
    pattern = re.compile(re.escape(tool_header) + r"(.*?)(?=\n\[|$)", re.DOTALL)
    match = pattern.search(content)
    
    if match:
        block_content = match.group(1)
        # Check for # test = or #test =
        if re.search(r"^\s*#\s*test\s*=", block_content, re.MULTILINE):
            return True
    return False

def get_latest_version(tool):
    try:
        result = subprocess.run(
            ["mise", "latest", tool], 
            capture_output=True, 
            text=True, 
            timeout=10
        )
        if result.returncode == 0:
            return result.stdout.strip()
    except subprocess.TimeoutExpired:
        pass
    except Exception:
        pass
    return None

def get_tool_version_output(tool, binary, flag, timeout_sec=5):
    try:
        # mise x --quiet -y <tool> -- <binary> <flag>
        # flag can be empty or multiple words (e.g. "help")
        args = [binary]
        if flag:
            args.extend(flag.split())
            
        cmd = ["mise", "x", "--quiet", "-y", tool, "--"] + args
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True, 
            timeout=timeout_sec 
        )
        return result.stdout, result.returncode
    except subprocess.TimeoutExpired:
        return "", -1
    except Exception as e:
        return "", -2

def install_tool(tool):
    try:
        cmd = ["mise", "install", "-y", f"{tool}@latest"]
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=120 # Give installation more time
        )
        return result.returncode == 0
    except subprocess.TimeoutExpired:
        return False
    except Exception:
        return False

def escape_string(s):
    return s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')

def log_failure_to_file(message):
    with open("test_generation.log", "a") as f:
        f.write(message + "\n")

def update_registry_in_place(tool, test_line, content):
    # Find [tools.tool] and insert test line
    tool_header = f"[tools.{tool}]"
    pattern = re.compile(re.escape(tool_header) + r"(.*?)(?=\n\[|$)", re.DOTALL)
    match = pattern.search(content)
    
    if match:
        block_content = match.group(0)
        # Check if test already exists (race condition or manual update?)
        if "test =" in block_content:
             return content
        
        # Append test line at end of block
        new_block = block_content.rstrip() + f"\n{test_line}\n"
        if not block_content.endswith("\n"):
             new_block += "\n" 
             
        # Replace the matched span
        start, end = match.span()
        new_content = content[:start] + new_block + content[end:]
        return new_content
    return content

def save_test(tool, binary, flag, test_str, content, comment=""):
    # cmd_used = f"{binary} {flag}".strip()
    # Handle flag properly for display
    cmd_parts = [binary]
    if flag:
        cmd_parts.append(flag)
    cmd_used = " ".join(cmd_parts)

    test_line = f'test = ["{cmd_used}", "{escape_string(test_str)}"]'
    if comment:
        test_line += f" # {comment}"
    
    content = update_registry_in_place(tool, test_line, content)
    with open("registry.toml", "w") as f:
        f.write(content)
    
    msg = f"MATCH: {tool} -> {test_str[:20]}... (via {cmd_used}) - UPDATED REGISTRY"
    print(f"  {msg}", file=sys.stderr)
    log_failure_to_file(msg)
    return content

def generate_tests():
    target_tool = None
    limit = None
    
    if len(sys.argv) > 1:
        arg = sys.argv[1]
        if arg.isdigit():
            limit = int(arg)
        else:
            target_tool = arg

    # Clear log file at start
    with open("test_generation.log", "w") as f:
        f.write("--- Test Generation Log ---\n")

    content = get_registry_content()
    data = parse_toml(content)
    tools = data.get("tools", {})
    
    missing_tests = []
    
    # Identify candidates
    for tool_name, tool_config in tools.items():
        if "test" not in tool_config:
            # User request: don't add tests for macos only tools
            if "os" in tool_config and tool_config["os"] == ["macos"]:
                continue
                
            if not is_test_commented_out(tool_name, content):
                if target_tool and tool_name != target_tool:
                    continue
                missing_tests.append(tool_name)
    
    if target_tool and not missing_tests:
        # Maybe it has a test or commented out?
        if target_tool in tools and "test" in tools[target_tool]:
             print(f"{target_tool} already has a test.", file=sys.stderr)
        elif target_tool not in tools:
             print(f"{target_tool} not found in registry.", file=sys.stderr)
        else:
             print(f"{target_tool} skipped (commented out test?).", file=sys.stderr)
        return

    print(f"Found {len(missing_tests)} tools missing tests.", file=sys.stderr)
    
    count = 0
    for i, tool in enumerate(sorted(missing_tests)):
        if limit and count >= limit:
            break
            
        print(f"[{i+1}/{len(missing_tests)}] Checking {tool}...", file=sys.stderr)
        
        # Determine binary name
        binary_candidates = [tool]
        tool_conf = tools.get(tool, {})
        if "aliases" in tool_conf:
            binary_candidates.extend(tool_conf["aliases"])
            
        latest_ver = get_latest_version(tool)
        if not latest_ver:
            msg = f"Failed to get latest version for {tool}"
            print(f"  {msg}", file=sys.stderr)
            log_failure_to_file(f"{tool}: {msg}")
            continue

        # Step 1: Install
        if not install_tool(tool):
            msg = f"INSTALL FAILED for {tool}"
            print(f"  {msg}", file=sys.stderr)
            log_failure_to_file(f"{tool}: {msg}")
            continue

        count += 1
        success = False
        
        # 1. Try different flags: --version, version, -v, -V
        flags = ["--version", "version", "-v", "-V"]
        
        timeouts_occurred = False
        fail_reasons = []

        # Strategy:
        # 1. Try standard version flags.
        # 2. If NO MATCH (but ran ok), continue.
        # 3. If TIMEOUT, mark as timeout.
        
        for binary in binary_candidates:
            for flag in flags:
                stdout, ret_code = get_tool_version_output(tool, binary, flag, timeout_sec=5)
                
                if ret_code == -1:
                    fail_reasons.append(f"{binary} {flag}: TIMEOUT")
                    timeouts_occurred = True
                    continue
                if ret_code != 0:
                     fail_reasons.append(f"{binary} {flag}: EXIT {ret_code}")
                     continue

                # Filter out mise logs and ANSI codes from stdout only
                clean_lines = []
                ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
                for line in stdout.splitlines():
                    if line.strip().startswith("mise "):
                        continue
                    clean_line = ansi_escape.sub('', line)
                    clean_lines.append(clean_line)
                clean_output = "\n".join(clean_lines)

                if latest_ver in clean_output:
                    # Found it!
                    match_line = ""
                    for line in clean_lines:
                        if latest_ver in line:
                            match_line = line
                            break
                    
                    test_str = match_line.replace(latest_ver, "{{version}}").strip()
                    if not test_str: test_str = "{{version}}"
                    
                    content = save_test(tool, binary, flag, test_str, content)
                    success = True
                    break
                else:
                    fail_reasons.append(f"{binary} {flag}: NO MATCH")
            
            if success: break
        
        if success: continue

        # Fallback 1: Try `help` if no timeouts yet (or reasonable to try)
        # User says: "if failed reason is not installation fail, run help subcommand"
        # Since install succeeded, we try help.
        
        if not success:
            print(f"  Standard flags failed. Trying 'help'...", file=sys.stderr)
            for binary in binary_candidates:
                stdout, ret_code = get_tool_version_output(tool, binary, "help", timeout_sec=5)
                
                if ret_code == -1:
                    fail_reasons.append(f"{binary} help: TIMEOUT")
                    timeouts_occurred = True
                    continue
                
                if ret_code == 0 and stdout:
                     # Use first line of output
                     lines = stdout.splitlines()
                     # Filter mise logs
                     valid_lines = [l for l in lines if not l.strip().startswith("mise ")]
                     if valid_lines:
                         first_line = valid_lines[0].strip()
                         # Remove ANSI
                         ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
                         clean_line = ansi_escape.sub('', first_line)
                         
                         if clean_line:
                             content = save_test(tool, binary, "help", clean_line, content)
                             success = True
                             break
            
        if success: continue
        
        # Fallback 2: "if the binary hangs... just test which"
        # Or if "no commands are offered" (help failed)
        # We assume if we have timeouts OR we couldn't get version/help, we fallback to `which`
        # User said: "then, if the binary hangs ... just test which"
        # I'll apply `which` fallback if we had timeouts OR if we have exhausted options and still no test.
        # "check other tests... for examples" -> `which ...` tests usually have empty output expected.
        
        if not success:
            if timeouts_occurred:
                comment = "hangs"
                print(f"  Falling back to 'which' ({comment})...", file=sys.stderr)
                
                binary = binary_candidates[0]
                content = save_test(tool, f"which {binary}", "", "", content, comment=comment)
                success = True
            else:
                # No timeout, just failed commands. Do NOT add test.
                pass

        if not success:
            reason_str = "; ".join(fail_reasons)
            print(f"  FAILED {tool}: {reason_str}", file=sys.stderr)
            log_failure_to_file(f"{tool} FAILED: {reason_str}")
    
    print("\n--- DONE ---\n")

if __name__ == "__main__":
    generate_tests()
