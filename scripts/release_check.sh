#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

skip_check=0
skip_test=0
expected_version=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-check)
      skip_check=1
      ;;
    --skip-test)
      skip_test=1
      ;;
    --expected-version)
      shift
      expected_version="${1:-}"
      if [[ -z "$expected_version" ]]; then
        echo "error: --expected-version requires a value" >&2
        exit 1
      fi
      ;;
    *)
      echo "usage: bash scripts/release_check.sh [--skip-check] [--skip-test] [--expected-version <version>]" >&2
      exit 1
      ;;
  esac
  shift
done

version_from_cargo() {
  awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml
}

version_from_lock() {
  awk '
    $0 == "[[package]]" { in_pkg=1; name=""; version="" }
    in_pkg && $0 ~ /^name = "codeclaw"$/ { name="codeclaw" }
    in_pkg && name == "codeclaw" && $0 ~ /^version = "/ {
      split($0, parts, "\"")
      print parts[2]
      exit
    }
  ' Cargo.lock
}

print_ok() {
  printf 'OK   %s\n' "$1"
}

print_warn() {
  printf 'WARN %s\n' "$1"
}

print_fail() {
  printf 'FAIL %s\n' "$1"
}

assert_file_contains() {
  local file="$1"
  local needle="$2"
  local label="$3"
  if grep -Fq -- "$needle" "$file"; then
    print_ok "$label"
  else
    print_fail "$label"
    failures=$((failures + 1))
  fi
}

assert_exists() {
  local path="$1"
  local label="$2"
  if [[ -e "$path" ]]; then
    print_ok "$label"
  else
    print_fail "$label"
    failures=$((failures + 1))
  fi
}

failures=0

cargo_version="$(version_from_cargo)"
lock_version="$(version_from_lock)"

if [[ -n "$expected_version" && "$cargo_version" != "$expected_version" ]]; then
  print_fail "Cargo.toml version matches expected version ($expected_version)"
  failures=$((failures + 1))
else
  print_ok "Cargo.toml version resolved to $cargo_version"
fi

if [[ "$cargo_version" == "$lock_version" ]]; then
  print_ok "Cargo.lock package version matches Cargo.toml"
else
  print_fail "Cargo.lock package version matches Cargo.toml"
  failures=$((failures + 1))
fi

assert_file_contains README.md "- Version: \`$cargo_version\`" "README release version matches $cargo_version"
assert_file_contains RELEASE.md "# CodeClaw Release \`v$cargo_version\`" "RELEASE.md title matches v$cargo_version"
assert_file_contains CHANGELOG.md "## $cargo_version -" "CHANGELOG contains $cargo_version heading"
assert_file_contains docs/user-guide.md "release \`$cargo_version\`" "User guide version matches $cargo_version"
assert_file_contains docs/operations-guide.md "CodeClaw \`$cargo_version\`" "Operations guide version matches $cargo_version"
assert_file_contains docs/project-delivery.md "| Version | \`$cargo_version\` |" "Project delivery version matches $cargo_version"
assert_file_contains docs/acceptance-use-cases.md "release \`$cargo_version\`" "Acceptance use cases version matches $cargo_version"
assert_file_contains docs/architecture.md "release \`$cargo_version\`" "Architecture doc version matches $cargo_version"
assert_file_contains docs/project-plan.md "release \`$cargo_version\`" "Project plan version matches $cargo_version"

assert_exists "docs/release-announcement-v$cargo_version.md" "Release announcement doc exists"
assert_exists "docs/upgrade-notes-v$cargo_version.md" "Upgrade notes doc exists"
assert_exists "docs/community-post-kit-v$cargo_version.md" "Community post kit exists"
assert_exists "docs/quickstart-card-v$cargo_version.md" "Quickstart card exists"
assert_exists "docs/faq.md" "FAQ exists"
assert_exists "docs/operator-runbook.md" "Operator runbook exists"
assert_exists "docs/customer-handover-checklist.md" "Customer handover checklist exists"
assert_exists "docs/incident-response-playbook.md" "Incident response playbook exists"
assert_exists "docs/ops-dashboard-spec.md" "Ops dashboard spec exists"
assert_exists "templates/customer-handover-template.md" "Customer handover template exists"
assert_exists "templates/im-gateway-rollout-template.md" "IM gateway rollout template exists"
assert_exists "templates/incident-report-template.md" "Incident report template exists"
assert_exists "scripts/handover_pack.sh" "handover_pack.sh exists"

if git diff --quiet && git diff --cached --quiet; then
  print_ok "Git worktree is clean"
else
  print_warn "Git worktree is not clean"
fi

if git rev-parse "v$cargo_version" >/dev/null 2>&1; then
  print_warn "Tag v$cargo_version already exists"
else
  print_ok "Tag v$cargo_version does not exist yet"
fi

if [[ "$skip_check" -eq 0 ]]; then
  cargo check
  print_ok "cargo check completed"
else
  print_warn "cargo check skipped"
fi

if [[ "$skip_test" -eq 0 ]]; then
  cargo test --quiet
  print_ok "cargo test --quiet completed"
else
  print_warn "cargo test --quiet skipped"
fi

if [[ "$failures" -gt 0 ]]; then
  echo
  echo "release check failed with $failures error(s)" >&2
  exit 1
fi

echo
echo "release check passed for version $cargo_version"
