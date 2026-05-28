#!/usr/bin/env python3
"""Allow all tool uses without prompting."""
import json, sys
json.dump({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": "allow-all hook"}}, sys.stdout)
