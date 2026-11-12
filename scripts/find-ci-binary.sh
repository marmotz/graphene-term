#!/usr/bin/env bash
# Print the id of the CI run whose `graphene-release` artifact was built from the
# same tree as <commit>, so the Release workflow can publish it without rebuilding.
# Prints nothing when there is no such run (direct push, branch not up to date when
# merged, failed CI, expired artifact); the caller then builds from source.
#
# Usage: scripts/find-ci-binary.sh <commit>
# Needs `gh` authenticated (GH_TOKEN in CI). Repository: $GITHUB_REPOSITORY, or the
# current checkout's.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <commit>" >&2
  exit 1
fi

commit=$1
artifact=graphene-release
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}

skip() {
  echo "no reusable CI binary: $1" >&2
  exit 0
}

tree_of() {
  gh api "repos/${repo}/commits/$1" --jq .commit.tree.sha
}

pr_head=$(gh api "repos/${repo}/commits/${commit}/pulls" \
  --jq '[.[] | select(.merged_at != null)][0].head.sha // empty')
[[ -n $pr_head ]] || skip "${commit} does not come from a merged pull request"

# The CI builds the pull request merged into its base. When the branch was up to
# date, that tree is the head's, and also the tree of the commit being released.
[[ $(tree_of "$commit") == $(tree_of "$pr_head") ]] \
  || skip "${commit} differs from the pull request head ${pr_head} (branch not up to date)"

run_id=$(gh run list --repo "$repo" --workflow ci.yml --commit "$pr_head" \
  --status success --limit 1 --json databaseId --jq '.[0].databaseId // empty')
[[ -n $run_id ]] || skip "no successful CI run for ${pr_head}"

available=$(gh api "repos/${repo}/actions/runs/${run_id}/artifacts" \
  --jq "[.artifacts[] | select(.name == \"${artifact}\" and (.expired | not))] | length")
[[ $available -gt 0 ]] || skip "CI run ${run_id} has no ${artifact} artifact"

echo "reusing ${artifact} from CI run ${run_id} (pull request head ${pr_head})" >&2
echo "$run_id"
