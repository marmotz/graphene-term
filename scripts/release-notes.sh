#!/usr/bin/env bash
# Build categorized release notes from conventional commit subjects since the previous tag.
#
# Usage: scripts/release-notes.sh <previous-tag-or-empty> <target-commit> <owner/repo>
set -euo pipefail

if [[ $# -ne 3 || ! $3 =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "usage: $0 <previous-tag-or-empty> <target-commit> <owner/repo>" >&2
  exit 1
fi

previous_tag=$1
target=$2
repository=$3
conventional_re='^([a-z]+)(\(([^)]*)\))?(!)?:[[:space:]]*(.+)$'
pull_request_suffix_re='^(.*)[[:space:]]\(#([0-9]+)\)$'
pull_request_ref_re='\(#([0-9]+)\)$'

git rev-parse --verify "${target}^{commit}" >/dev/null
if [[ -n $previous_tag ]]; then
  git rev-parse --verify "${previous_tag}^{commit}" >/dev/null
  range="${previous_tag}..${target}"
else
  range=$target
fi

novelties=()
modifications=()
fixes=()

while IFS= read -r -d '' record; do
  commit=${record%%$'\t'*}
  subject=${record#*$'\t'}
  kind=other
  scope=""
  title=$subject

  if [[ $subject =~ $conventional_re ]]; then
    kind=${BASH_REMATCH[1]}
    scope=${BASH_REMATCH[3]:-}
    title=${BASH_REMATCH[5]}
  elif [[ $subject =~ ^([Ff]ix|[Bb]ugfix)([[:space:]:]+)(.+)$ ]]; then
    kind=fix
  fi

  if [[ $title =~ $pull_request_suffix_re ]]; then
    title=${BASH_REMATCH[1]}
  fi
  if [[ -n $scope ]]; then
    title="**${scope}:** ${title}"
  fi

  if [[ $subject =~ $pull_request_ref_re ]]; then
    reference="[#${BASH_REMATCH[1]}](https://github.com/${repository}/pull/${BASH_REMATCH[1]})"
  else
    reference="[${commit:0:7}](https://github.com/${repository}/commit/${commit})"
  fi
  line="- ${title} (${reference})"

  case $kind in
    feat) novelties+=("$line") ;;
    fix) fixes+=("$line") ;;
    *) modifications+=("$line") ;;
  esac
done < <(git log --no-merges -z --format='%H%x09%s' "$range")

print_section() {
  local title=$1
  shift
  (($# > 0)) || return 0
  printf '## %s\n\n' "$title"
  printf '%s\n' "$@"
  printf '\n'
}

if ((${#novelties[@]} + ${#modifications[@]} + ${#fixes[@]} == 0)); then
  echo "Aucun changement depuis la release précédente."
  exit 0
fi

if ((${#novelties[@]})); then
  print_section "Nouveautés" "${novelties[@]}"
fi
if ((${#modifications[@]})); then
  print_section "Modifications" "${modifications[@]}"
fi
if ((${#fixes[@]})); then
  print_section "Correctifs" "${fixes[@]}"
fi
