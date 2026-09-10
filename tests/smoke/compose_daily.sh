#!/bin/sh
set -eu

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'required command is missing: %s\n' "$1" >&2
    exit 1
  }
}

for command_name in curl docker jq mktemp sha256sum; do
  require_command "$command_name"
done

smoke_root=$(mktemp -d "${TMPDIR:-/tmp}/log-inbox-compose-smoke.XXXXXX")
project="log-inbox-smoke-$$"
export LOG_INBOX_SMOKE_UID="$(id -u)"
export LOG_INBOX_SMOKE_GID="$(id -g)"
export LOG_INBOX_SMOKE_DATA_DIR="$smoke_root/data"
export LOG_INBOX_SMOKE_WORKSPACE_DIR="$smoke_root/workspace"
compose="docker compose -p $project -f docker-compose.smoke.yml"
mkdir -p "$LOG_INBOX_SMOKE_DATA_DIR" "$LOG_INBOX_SMOKE_WORKSPACE_DIR/Journal"

cleanup() {
  $compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  rm -rf "$smoke_root"
}
trap cleanup EXIT INT TERM

local_date=$(date -u +%F)
target="$LOG_INBOX_SMOKE_WORKSPACE_DIR/Journal/$local_date.md"
printf '%s\n' '---' 'title: Existing owner note' '---' '' 'Owner content before the managed block.' >"$target"

$compose up --build --detach --wait
collector_port=$($compose port collector 8787 | awk -F: 'END {print $NF}')
daily_port=$($compose port daily 8788 | awk -F: 'END {print $NF}')
collector_url="http://127.0.0.1:$collector_port"
daily_url="http://127.0.0.1:$daily_port"
daily_headers="Host: 127.0.0.1:8788"
origin_header="Origin: http://127.0.0.1:8788"
cookie_jar="$smoke_root/cookies"

event_body=$(jq -n --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '{source:"smoke/compose",level:"info",timestamp:$timestamp,message:"Completed the live Compose smoke workflow.",metadata:{task_id:"compose-smoke",event_type:"complete",status:"succeeded",repo:"log-inbox",tests:["live Compose smoke"]}}')
curl -fsS --max-time 10 "$collector_url/v1/logs" \
  -H 'Authorization: Bearer smoke-ingest-key' \
  -H 'Content-Type: application/json' \
  --data-binary "$event_body" >/dev/null

login=$(curl -fsS --max-time 10 "$daily_url/api/v2/auth/login" \
  -H "$daily_headers" -H "$origin_header" -H 'Content-Type: application/json' \
  -c "$cookie_jar" \
  --data-binary '{"owner_secret":"smoke-owner-secret-at-least-20-bytes"}')
csrf=$(printf '%s' "$login" | jq -er '.csrf_token')

settings=$(jq -n '{timezone:"UTC",daily_root:"Journal",daily_pattern:"{date}.md",template_path:null,link_style:"markdown"}')
preview=$(curl -fsS --max-time 10 "$daily_url/api/v2/settings/workspace/preview" \
  -H "$daily_headers" -H "$origin_header" -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -b "$cookie_jar" --data-binary "$settings")
save=$(jq -n --argjson settings "$settings" --arg digest "$(printf '%s' "$preview" | jq -er '.preview_digest')" '{settings:$settings,preview_digest:$digest,expected_profile_id:null,expected_updated_at:null}')
curl -fsS --max-time 10 -X PUT "$daily_url/api/v2/settings/workspace" \
  -H "$daily_headers" -H "$origin_header" -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -b "$cookie_jar" --data-binary "$save" >/dev/null

generate_body="$smoke_root/generate.json"
generate_status=$(curl -sS --max-time 20 -o "$generate_body" -w '%{http_code}' "$daily_url/api/v2/daily/$local_date/generate" \
  -H "$daily_headers" -H "$origin_header" -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -b "$cookie_jar" --data-binary '{}')
if [ "$generate_status" -lt 200 ] || [ "$generate_status" -ge 300 ]; then
  printf 'Daily generation failed with HTTP %s: ' "$generate_status" >&2
  jq -c . "$generate_body" >&2 || sed -n '1,20p' "$generate_body" >&2
  exit 1
fi
revision=$(jq -c . "$generate_body")
revision_id=$(printf '%s' "$revision" | jq -er '.id')
day=$(curl -fsS --max-time 10 "$daily_url/api/v2/daily/$local_date" -H "$daily_headers" -b "$cookie_jar")
event_id=$(printf '%s' "$day" | jq -er '.current_snapshot.event_ids[0]')
decision=$(jq -n --arg revision "$revision_id" '{expected_revision_id:$revision,disposition:"include",related_event_id:null,reason:"Compose smoke evidence"}')
curl -fsS --max-time 10 -X PUT "$daily_url/api/v2/daily/$local_date/evidence/$event_id" \
  -H "$daily_headers" -H "$origin_header" -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -b "$cookie_jar" --data-binary "$decision" >/dev/null

apply_preview=$(curl -fsS --max-time 10 "$daily_url/api/v2/daily/$local_date/apply-preview" -H "$daily_headers" -b "$cookie_jar")
apply=$(printf '%s' "$apply_preview" | jq '{expected_revision_id:.revision_id,expected_revision_content_hash:.revision_content_hash,destination_path:.destination_path,expected_old_block_hash:.expected_old_block_hash,intended_new_block_hash:.intended_new_block_hash,expected_target_exists:.expected_target_exists,expected_original_content_hash:.expected_original_content_hash,expected_updated_content_hash:.updated_content_hash}')
curl -fsS --max-time 10 "$daily_url/api/v2/daily/$local_date/apply" \
  -H "$daily_headers" -H "$origin_header" -H "X-CSRF-Token: $csrf" -H 'Content-Type: application/json' \
  -b "$cookie_jar" --data-binary "$apply" | jq -e '.operation.state == "finalized"' >/dev/null

grep -F 'Owner content before the managed block.' "$target" >/dev/null
grep -F 'Validated the live Daily workflow.' "$target" >/dev/null
test "$(grep -c 'log-inbox:daily:.*:begin' "$target")" -eq 1
applied_hash=$(sha256sum "$target" | awk '{print $1}')

$compose restart daily >/dev/null
daily_port=$($compose port daily 8788 | awk -F: 'END {print $NF}')
daily_url="http://127.0.0.1:$daily_port"
attempt=1
while [ "$attempt" -le 30 ]; do
  if curl -fsS --max-time 2 "$daily_url/health" -H "$daily_headers" >/dev/null 2>&1; then
    break
  fi
  if [ "$attempt" -eq 30 ]; then
    printf 'Daily service did not recover after restart\n' >&2
    $compose logs --no-color daily >&2 || true
    exit 1
  fi
  sleep 1
  attempt=$((attempt + 1))
done
test "$(sha256sum "$target" | awk '{print $1}')" = "$applied_hash"
curl -fsS --max-time 10 "$daily_url/api/v2/daily/$local_date" -H "$daily_headers" -b "$cookie_jar" \
  | jq -e '.apply_status.state == "finalized" and .day.review_status == "applied"' >/dev/null

printf 'Compose Daily smoke passed for %s\n' "$local_date"
