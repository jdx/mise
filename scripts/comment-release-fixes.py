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


def stable_releases_after(tag, limit=10):
    """Up to `limit` stable releases listed after `tag`, newest first.

    Pages through the whole list, since a manual backfill can name any tag.
    """
    found = False
    after = []
    page = 1
    while len(after) < limit:
        releases = json.loads(
            gh("api", f"repos/{REPO}/releases?per_page=100&page={page}")
        )
        if not releases:
            break
        for release in releases:
            if release["draft"] or release["prerelease"]:
                continue
            if release["tag_name"] == tag:
                found = True
            elif found:
                after.append(release["tag_name"])
        page += 1
    if not found:
        raise SystemExit(f"{tag} is not a stable release")
    return after[:limit]


def previous_release(tag):
    """The nearest earlier release that `tag` actually contains.

    The release list is ordered by creation, not by history, so a release that
    was drafted early and published late can sit out of order. Among the
    nearest listed releases, take the ancestor of `tag` with the fewest commits
    between it and `tag`, rather than trusting the list order. Scanning the
    whole list would cost one request per release on every run, and an
    out-of-order release lands within a few entries of its neighbors.
    """
    best = None
    for candidate in stable_releases_after(tag):
        try:
            status, ahead_by = gh(
                "api",
                f"repos/{REPO}/compare/{candidate}...{tag}?per_page=1",
                "--jq",
                '[.status, (.ahead_by | tostring)] | join(" ")',
            ).split()
        except subprocess.CalledProcessError as err:
            # Only a missing tag is permanent. Any other failure may be
            # transient, and skipping the nearest release would pick an older
            # base and announce fixes that shipped earlier. Fail instead; the
            # markers make a re-run safe.
            if "404" not in err.stderr:
                raise
            print(f"::warning::could not compare {candidate}...{tag}: tag not found")
            continue
        if status == "ahead" and (best is None or int(ahead_by) < best[0]):
            best = (int(ahead_by), candidate)
    if not best:
        raise SystemExit(f"no earlier release found that {tag} contains")
    return best[1]


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
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      body
      closingIssuesReferences(first: 100, after: $cursor) {
        pageInfo { hasNextPage endCursor }
        nodes { number state repository { nameWithOwner } }
      }
    }
  }
}"""

DISCUSSION_QUERY = """
query($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    discussion(number: $number) {
      id
      closed
      comments(first: 100, after: $cursor) {
        pageInfo { hasNextPage endCursor }
        nodes { body author { login } }
      }
    }
  }
}"""

# The comments are posted with GITHUB_TOKEN. REST reports that author as
# "github-actions[bot]" and GraphQL as "github-actions".
BOT_LOGINS = {"github-actions[bot]", "github-actions"}


def issue_targets(pr):
    """Issues a PR closed, plus discussions it closed by keyword."""
    nodes = []
    cursor = None
    while True:
        variables = {"owner": OWNER, "name": NAME, "number": pr}
        if cursor:
            variables["cursor"] = cursor
        pull = graphql(PR_QUERY, **variables)["repository"]["pullRequest"]
        refs = pull["closingIssuesReferences"]
        nodes += refs["nodes"]
        if not refs["pageInfo"]["hasNextPage"]:
            break
        cursor = refs["pageInfo"]["endCursor"]
    issues = {
        node["number"]
        for node in nodes
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
    # Only our own comments count: anyone can paste the marker into theirs.
    comments = gh(
        "api",
        "--paginate",
        f"repos/{REPO}/issues/{number}/comments",
        "--jq",
        '.[] | select(.user.login == "github-actions[bot]") | .body',
    )
    if marker(tag) in comments:
        print(f"skip issue #{number}: already commented for {tag}")
        return
    post(
        f"issue #{number}",
        lambda body: gh("api", f"repos/{REPO}/issues/{number}/comments", "-F", "body=@-", stdin=body),
        tag,
        pr,
    )


def discussion_comments(number):
    """(id, closed, comments) for a discussion, or None if it is not one."""
    cursor = None
    comments = []
    while True:
        variables = {"owner": OWNER, "name": NAME, "number": number}
        if cursor:
            variables["cursor"] = cursor
        discussion = graphql(DISCUSSION_QUERY, **variables)["repository"]["discussion"]
        if not discussion:
            return None
        page = discussion["comments"]
        comments += page["nodes"]
        if not page["pageInfo"]["hasNextPage"]:
            return discussion["id"], discussion["closed"], comments
        cursor = page["pageInfo"]["endCursor"]


def comment_on_discussion(number, tag, pr):
    found = discussion_comments(number)
    if not found:
        return  # a PR or issue number, not a discussion
    discussion_id, closed, comments = found
    if not closed:
        # Naming a discussion with a closing keyword does not close it if the
        # discussion was reopened or the keyword was a loose match.
        print(f"skip discussion #{number}: still open")
        return
    if any(
        marker(tag) in c["body"] and (c["author"] or {}).get("login") in BOT_LOGINS
        for c in comments
    ):
        print(f"skip discussion #{number}: already commented for {tag}")
        return
    mutation = (
        "mutation($id: ID!, $body: String!) "
        "{ addDiscussionComment(input: {discussionId: $id, body: $body}) { comment { id } } }"
    )
    post(
        f"discussion #{number}",
        lambda body: graphql(mutation, id=discussion_id, body=body),
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
