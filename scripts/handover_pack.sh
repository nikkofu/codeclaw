#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

version="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
commit="$(git rev-parse --short HEAD)"
timestamp="$(date +"%Y%m%d-%H%M%S")"
output_dir="${1:-tmp/handover-pack/v${version}-${timestamp}}"

mkdir -p "$output_dir/docs" "$output_dir/templates"

files=(
  "README.md"
  "RELEASE.md"
  "CHANGELOG.md"
  "docs/project-delivery.md"
  "docs/user-guide.md"
  "docs/operations-guide.md"
  "docs/acceptance-use-cases.md"
  "docs/quickstart-card-v${version}.md"
  "docs/release-announcement-v${version}.md"
  "docs/upgrade-notes-v${version}.md"
  "docs/community-post-kit-v${version}.md"
  "docs/faq.md"
  "docs/operator-runbook.md"
  "docs/customer-handover-checklist.md"
  "docs/incident-response-playbook.md"
  "docs/im-gateway-rollout-checklist.md"
  "docs/ops-dashboard-spec.md"
  "templates/customer-handover-template.md"
  "templates/im-gateway-rollout-template.md"
  "templates/incident-report-template.md"
)

copied_files=()
missing_files=()

for file in "${files[@]}"; do
  if [[ -f "$file" ]]; then
    mkdir -p "$output_dir/$(dirname "$file")"
    cp "$file" "$output_dir/$file"
    copied_files+=("$file")
  else
    missing_files+=("$file")
  fi
done

manifest="$output_dir/MANIFEST.md"
{
  echo "# CodeClaw Handover Pack"
  echo
  echo "- Version: \`$version\`"
  echo "- Commit: \`$commit\`"
  echo "- Generated at: \`$(date +"%Y-%m-%d %H:%M:%S %z")\`"
  echo "- Source repository: \`$repo_root\`"
  echo "- Output directory: \`$output_dir\`"
  echo
  echo "## Included Files"
  echo
  for file in "${copied_files[@]}"; do
    echo "- \`$file\`"
  done

  if [[ "${#missing_files[@]}" -gt 0 ]]; then
    echo
    echo "## Missing Files"
    echo
    for file in "${missing_files[@]}"; do
      echo "- \`$file\`"
    done
  fi
} > "$manifest"

checksums="$output_dir/checksums.txt"
: > "$checksums"
for file in "${copied_files[@]}"; do
  shasum -a 256 "$output_dir/$file" >> "$checksums"
done
shasum -a 256 "$manifest" >> "$checksums"

echo "handover pack ready: $output_dir"
echo "version: $version"
echo "commit: $commit"
echo "files copied: ${#copied_files[@]}"
if [[ "${#missing_files[@]}" -gt 0 ]]; then
  echo "missing files: ${#missing_files[@]}"
fi
