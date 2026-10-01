#!/usr/bin/env python3
"""Tell issue and discussion authors which release shipped their fix.

Issues close when their PR merges, which is days before the release. This posts
one "Fixed in vX.Y.Z" comment on each issue or discussion that a PR in the
release closed, so the reporter knows what to upgrade to.

Usage: comment-release-fixes.py TAG   (DRY_RUN=1 prints instead of posting)

Needs GH_TOKEN and GITHUB_REPOSITORY. Best effort: a failure on one item is
reported as a warning and never fails the run.
"""

import json
import os
import re
import subprocess
import sys

REPO = os.environ.get("GITHUB_REPOSITORY", "jdx/mise")
OWNER, NAME = REPO.split("/")
DRY_RUN = os.environ.get("DRY_RUN") == "1"

# The same keywords link-discussion-action closes a discussion on. `Refs #N`
# only links, so it does not mean the discussion was resolved.
CLOSING_REF = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+(?:discussion\s+)?#(\d+)\b",
    re.IGNORECASE,
)
SQUASH_PR = re.compile(r"\(#(\d+)\)\s*$")


def gh(*args, stdin=None):
    return subprocess.run(
        ["gh", *args], input=stdin, text=True, capture_output=True, check=True
    ).stdout


def graphql(query, **variables):
    args = ["api", "graphql", "-f", f"query={query}"]
    for key, value in variables.items():
        flag = "-F" if isinstance(value, int) else "-f"
        args += [flag, f"{key}={value}"]
    return json.loads(gh(*args))["data"]


def previous_release(tag):
    """The stable release published just before `tag`, in publication order."""
    releases = json.loads(gh("api", f"repos/{REPO}/releases?per_page=50"))
    stable = [r["tag_name"] for r in releases if not r["draft"] and not r["prerelease"]]
    if tag not in stable:
        raise SystemExit(f"{tag} is not among the {len(stable)} latest stable releases")
    index = stable.index(tag) + 1
    if index >= len(stable):
        raise SystemExit(f"no release before {tag}")
    return stable[index]


def release_prs(base, tag):
    out = gh(
        "api",
        "--paginate",
        f"repos/{REPO}/compare/{base}...{tag}?per_page=100",
        "--jq",
        ".commits[].commit.message | split(\"\\n\")[0]",
    )
    prs = []
    for subject in out.splitlines():
        match = SQUASH_PR.search(subject)
        if match and int(match.group(1)) not in prs:
            prs.append(int(match.group(1)))
    return prs


def strip_noise(body):
    """Drop HTML comments and code, where PR templates keep example keywords."""
    body = re.sub(r"<!--.*?-->", "", body or "", flags=re.DOTALL)
    return re.sub(r"```.*?```", "", body, flags=re.DOTALL)


PR_QUERY = """
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      body
      closingIssuesReferences(first: 20) {
        nodes { number state repository { nameWithOwner } }
      }
    }
  }
}"""

DISCUSSION_QUERY = """
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    discussion(number: $number) {
      id
      closed
      comments(last: 100) { nodes { body } }
    }
  }
}"""


def issue_targets(pr):
    """Issues a PR closed, plus discussions it closed by keyword."""
    data = graphql(PR_QUERY, owner=OWNER, name=NAME, number=pr)["repository"]
    pull = data["pullRequest"]
    issues = {
        node["number"]
        for node in pull["closingIssuesReferences"]["nodes"]
        if node["state"] == "CLOSED" and node["repository"]["nameWithOwner"] == REPO
    }
    # A number that is not an issue may be a discussion; `discussion(number:)`
    # returns null for one that does not exist, so unknown numbers drop out.
    candidates = {
        int(n) for n in CLOSING_REF.findall(strip_noise(pull["body"]))
    } - issues
    return sorted(issues), sorted(candidates)


def marker(tag):
    return f"<!-- mise-fixed-in:{tag} -->"


def comment_body(tag, pr):
    url = f"https://github.com/{REPO}/releases/tag/{tag}"
    return (
        f"{marker(tag)}\n"
        f"Fixed in [{tag}]({url}) by #{pr}. "
        "Update with `mise self-update` or see the [install docs](https://mise.jdx.dev/installing-mise.html)."
    )


def post(label, send, tag, pr):
    if DRY_RUN:
        print(f"[dry run] would comment on {label} (from #{pr})")
        return
    send(comment_body(tag, pr))
    print(f"commented on {label} (from #{pr})")


def comment_on_issue(number, tag, pr):
    comments = gh("api", "--paginate", f"repos/{REPO}/issues/{number}/comments", "--jq", ".[].body")
    if marker(tag) in comments:
        print(f"skip issue #{number}: already commented for {tag}")
        return
    post(
        f"issue #{number}",
        lambda body: gh("api", f"repos/{REPO}/issues/{number}/comments", "-F", "body=@-", stdin=body),
        tag,
        pr,
    )


def comment_on_discussion(number, tag, pr):
    discussion = graphql(DISCUSSION_QUERY, owner=OWNER, name=NAME, number=number)["repository"]["discussion"]
    if not discussion:
        return  # a PR or issue number, not a discussion
    if any(marker(tag) in c["body"] for c in discussion["comments"]["nodes"]):
        print(f"skip discussion #{number}: already commented for {tag}")
        return
    mutation = (
        "mutation($id: ID!, $body: String!) "
        "{ addDiscussionComment(input: {discussionId: $id, body: $body}) { comment { id } } }"
    )
    post(
        f"discussion #{number}",
        lambda body: graphql(mutation, id=discussion["id"], body=body),
        tag,
        pr,
    )


def main():
    if len(sys.argv) != 2 or not re.fullmatch(r"v[0-9][0-9A-Za-z._+-]*", sys.argv[1]):
        raise SystemExit("usage: comment-release-fixes.py vX.Y.Z")
    tag = sys.argv[1]
    base = previous_release(tag)
    prs = release_prs(base, tag)
    print(f"{tag}: {len(prs)} PRs since {base}")
    for pr in prs:
        try:
            issues, candidates = issue_targets(pr)
        except Exception as err:  # noqa: BLE001 - best effort per PR
            print(f"::warning::could not read #{pr}: {err}")
            continue
        for number in issues:
            try:
                comment_on_issue(number, tag, pr)
            except Exception as err:  # noqa: BLE001
                print(f"::warning::could not comment on issue #{number}: {err}")
        for number in candidates:
            try:
                comment_on_discussion(number, tag, pr)
            except Exception as err:  # noqa: BLE001
                print(f"::warning::could not comment on discussion #{number}: {err}")


if __name__ == "__main__":
    main()
