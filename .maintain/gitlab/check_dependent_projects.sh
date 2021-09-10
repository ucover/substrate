#!/usr/bin/env bash
#
# Ensure that a PR does not introduce downstream breakages on this project's dependents by
# performing checks using this branch's code. If dependents are specified as companions, they are
# patched to use the code we have in this branch; otherwise, we run the the checks against their
# default branch.

# Companion dependents are extracted from the PR's description when lines conform to the following
# formats:
# [cC]ompanion: $organization/$repo#567
# [cC]ompanion: $repo#567

#shellcheck source=../common/lib.sh
. "$(dirname "${0}")/../common/lib.sh"

set -eu -o pipefail
shopt -s inherit_errexit

echo "
check_dependent_projects
========================

This check ensures that this project's dependents do not suffer downstream breakages from new code
changes.
"

die() {
  if [ "${1:-}" ]; then
    echo "$1" >&2
  fi
  exit 1
}

# Those are the dependents which will unconditionally be checked even if no companions are specified
# in the pull request's description
dependents=("polkadot" "cumulus" "grandpa-bridge-gadget")

this_repo="substrate"
this_repo_diener_patch="--substrate"
this_repo_dir="$PWD"
org="paritytech"

# Set the user name and email to make merging work
git config --global user.name 'CI system'
git config --global user.email '<>'
git config --global pull.rebase false

# Merge master into our branch so that the compilation takes into account how the code is going to
# going to perform when the code for this pull request lands on the target branch (à la pre-merge
# pipelines).
# Note that the target branch might not actually be master, but we default to it in the assumption
# of the common case. This could be refined in the future.
git pull origin master

our_crates=()
our_crates_source="git+https://github.com/paritytech/substrate"
discover_our_crates() {
  # workaround for early exits not being detected in command substitution
  # https://unix.stackexchange.com/questions/541969/nested-command-substitution-does-not-stop-a-script-on-a-failure-even-if-e-and-s
  local last_line

  local found
  while IFS= read -r crate; do
    last_line="$crate"
    # for avoiding duplicate entries
    for our_crate in "${our_crates[@]}"; do
      if [ "$crate" == "$our_crate" ]; then
        found=true
        break
      fi
    done
    if [ "${found:-}" ]; then
      unset found
    else
      our_crates+=("$crate")
    fi
  # dependents with {"source": null} are the ones we own, hence the getpath($p)==null in the jq
  # script below
  done < <(cargo metadata --quiet --format-version=1 | jq -r '
    . as $in |
    paths |
    select(.[-1]=="source" and . as $p | $in | getpath($p)==null) as $path |
    del($path[-1]) as $path |
    $in | getpath($path + ["name"])
  ')
  if [ -z "${last_line+_}" ]; then
    die "No lines were read for cargo metadata of $PWD (some error probably occurred)"
  fi
}
discover_our_crates

match_their_crates() {
  local target_name="$(basename "$PWD")"
  local crates_not_found=()
  local found

  # workaround for early exits not being detected in command substitution
  # https://unix.stackexchange.com/questions/541969/nested-command-substitution-does-not-stop-a-script-on-a-failure-even-if-e-and-s
  local last_line

  # output will be consumed in the format:
  #   crate
  #   source
  #   crate
  #   ...
  local next="crate"
  while IFS= read -r line; do
    last_line="$line"
    case "$next" in
      crate)
        next="source"
        crate="$line"
      ;;
      source)
        next="crate"
        if [ "$line" == "$our_crates_source" ] || [[ "$line" == "$our_crates_source?"* ]]; then
          for our_crate in "${our_crates[@]}"; do
            if [ "$our_crate" == "$crate" ]; then
              found=true
              break
            fi
          done
          if [ "${found:-}" ]; then
            unset found
          else
            # for avoiding duplicate entries
            for crate_not_found in "${crates_not_found[@]}"; do
              if [ "$crate_not_found" == "$crate" ]; then
                found=true
                break
              fi
            done
            if [ "${found:-}" ]; then
              unset found
            else
              crates_not_found+=("$crate")
            fi
          fi
        fi
      ;;
      *)
        die "ERROR: Unknown state $next"
      ;;
    esac
  done < <(cargo metadata --quiet --format-version=1 | jq -r '
    . as $in |
    paths(select(type=="string")) |
    select(.[-1]=="source") as $source_path |
    del($source_path[-1]) as $path |
    [$in | getpath($path + ["name"]), getpath($path + ["source"])] |
    .[]
  ')
  if [ -z "${last_line+_}" ]; then
    die "No lines were read for cargo metadata of $PWD (some error probably occurred)"
  fi

  if [ "${crates_not_found[@]}" ]; then
    echo -e "Errors during crate matching\n"
    printf "Failed to detect our crate \"%s\" referenced in $target_name\n" "${crates_not_found[@]}"
    echo -e "\nNote: this error generally happens if you have deleted or renamed a crate and did not update it in $target_name. Consider opening a companion pull request on $target_name and referencing it in this pull request's description like:\n$target_name companion: [your companion PR here]"
    die "Check failed"
  fi
}

patch_and_check_dependent() {
  match_their_crates
  diener patch --crates-to-patch "$this_repo_dir" $this_repo_diener_patch --path "Cargo.toml"
  cargo test --all
}

companions_found=()
companion_errors=()
process_companion_pr() {
  local companion_repo, pr_number

  # e.g. https://github.com/paritytech/polkadot/pull/123
  # or   polkadot#123
  local companion_expr="$1"
  if [[ "$companion_expr" =~ ^https://github\.com/$org/([^/]+)/pull/([[:digit:]]+) ]] ||
    [[ "$companion_expr" =~ ^([^#]+)#([[:digit:]]+) ]]; then
    companion_repo="${BASH_REMATCH[1]}"
    pr_number="${BASH_REMATCH[2]}"
  else
    die "Companion PR description had invalid format or did not belong to organization $org: $companion_expr"
  fi

  companions_found+=("$companion_repo")

  read -r mergeable pr_head_ref pr_head_sha < <(curl -sSL "$api_base/repos/$org/$companion_repo/pulls/$pr_number" | jq -r "\(mergeable) \(.head.ref) \(.head.sha)")

  local expected_mergeable=true
  if [ "$mergeable" != "$expected_mergeable" ]; then
    companion_errors+=("Github API says $companion_expr is not mergeable (checked at $(date))")
    return
  fi

  if [ ! -e "$companion_repo" ]; then
    git clone --depth 1 "https://github.com/$org/$companion_repo.git"
  fi
  pushd "$companion_repo" >/dev/null
  git fetch origin "pull/$pr_number/head:$pr_head_ref"
  git checkout "$pr_head_sha"

  echo "running checks for the companion $companion_expr of $companion_repo"
  patch_and_check_dependent

  popd >/dev/null
}

process_companions() {
  if [[ ! "$CI_COMMIT_REF_NAME" =~ ^[0-9]\+$ ]]; then
    return
  fi

  echo "this is pull request number $CI_COMMIT_REF_NAME"

  # workaround for early exits not being detected in command substitution
  # https://unix.stackexchange.com/questions/541969/nested-command-substitution-does-not-stop-a-script-on-a-failure-even-if-e-and-s
  local last_line
  while IFS= read -r line; do
    last_line="$line"
    if [[ ! "$line" =~ [cC]ompanion:[[:space:]]*(.+) ]]; then
      continue
    fi

    echo "detected companion in PR description: ${BASH_REMATCH[1]}"
    process_companion_pr "${BASH_REMATCH[1]}"
  done < <(curl -sSL -H "${github_header}" "$api_base/$this_repo/pulls/$CI_COMMIT_REF_NAME" | jq -r ".body")
  if [ -z "${last_line+_}" ]; then
    die "No lines were read for the description of PR $pr_number (some error probably occurred)"
  fi
}
process_companions

for dep in "${dependents[@]}"; do
  # if the dependent has already been checked as a companion, there's no point to test their
  # default branch
  for companion in "${companions_found[@]}"; do
    if [ "$dep" = "$companion" ]; then
      continue 2
    fi
  done

  echo "running checks for the default branch of $dep"

  if [ ! -e "$dep" ]; then
    git clone --depth 1 "https://github.com/$org/$dep.git"
  fi
  pushd "$dep" >/dev/null

  patch_and_check_dependent

  popd >/dev/null
done

if [ "${companion_errors[@]}" ]; then
  echo
  printf "%s\n" "${companion_errors[@]}"
fi
