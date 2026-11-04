/** Where schedules can come from besides a bundle: what to run to fetch them from that tool's API. */
export interface MigrationSource {
  key: string;
  label: string;
  script: string;
  /** The page of the docs that says what comes over, under the language's root. */
  docsPath: string;
}

const OPSGENIE_SCRIPT = `API=https://api.opsgenie.com   # EU accounts: https://api.eu.opsgenie.com
DAYS=200

get() { curl -fsS -H "Authorization: GenieKey $OPSGENIE_API_KEY" "$API$1"; }

get /v2/schedules | jq -r '.data[].id' | while read -r id; do
  get "/v2/schedules/$id"
  get "/v2/schedules/$id/timeline?expand=base&interval=$DAYS&intervalUnit=days"
done | jq -s '{
  data: map(.data | select(has("rotations"))),
  timelines: map(.data | select(has("baseTimeline"))
    | {_parent, startDate, endDate, baseTimeline}
    | .baseTimeline.rotations[].periods |= map({startDate, endDate, type, recipient}))
}' > opsgenie-schedules.json
`;

const PAGERDUTY_SCRIPT = `API=https://api.pagerduty.com   # EU accounts: https://api.eu.pagerduty.com
DAYS=200
since=$(jq -rn 'now | todate')
until=$(jq -rn "now + $DAYS * 86400 | todate")

get() {
  curl -fsSg -H "Authorization: Token token=$PAGERDUTY_API_KEY" \\
    -H 'Accept: application/vnd.pagerduty+json;version=2' "$API$1"
}
pages() {
  offset=0
  while page=$(get "$1?limit=100&offset=$offset"); do
    printf '%s\\n' "$page"
    [ "$(printf '%s' "$page" | jq .more)" = true ] || break
    offset=$((offset + 100))
  done
}
shift_based=$(pages /v3/schedules)

{
  pages /users
  printf '%s\\n' "$shift_based"
  printf '%s\\n' "$shift_based" | jq -r '.schedules[].id' | while read -r id; do
    get "/v3/schedules/$id?since=$since&until=$until&include[]=final_schedule"
  done
  pages /schedules | jq -r '.schedules[].id' | while read -r id; do
    get "/schedules/$id?since=$since&until=$until&include[]=final_schedule&include[]=overrides_subschedule"
  done
} | jq -s --arg since "$since" --arg until "$until" '{
  since: $since,
  until: $until,
  users: (map(.users // []) | add // [] | map({id, email})),
  shift_based_schedules: (map(.schedules // []) | add // [] | map({id, type, summary})),
  schedules: map(.schedule // empty
    | del(.users, .escalation_policies)
    | if .type == "schedule_v3" then
        .final_schedule.computed_shift_assignments[]? |= {start_time, end_time, member, source: {type: .source.type}}
      else
        del(.schedule_layers[].rendered_schedule_entries)
        | .final_schedule.rendered_schedule_entries |= map({start, end, user: (.user | select(.) | {id})})
        | .overrides_subschedule.rendered_schedule_entries |= map({start, end})
      end)
}' > pagerduty-schedules.json
`;

export const SCHEDULE_SOURCES: readonly MigrationSource[] = [
  {
    key: 'opsgenie',
    label: 'Opsgenie',
    script: OPSGENIE_SCRIPT,
    docsPath: 'alerting/schedule-migration#opsgenie',
  },
  {
    key: 'pagerduty',
    label: 'PagerDuty',
    script: PAGERDUTY_SCRIPT,
    docsPath: 'alerting/schedule-migration#pagerduty',
  },
];
