---
name: init
description: Write the knowledge of this organization or team from scratch - scan what is already set up, interview the person topic by topic, and write each file only after they confirm it.
argument-hint: [what to focus on]
version: "1"
allowed-tools: list_knowledge, read_knowledge, edit_knowledge, ask_user, todo_write, list_streams, get_stream_schema, list_apm_services, get_service_topology, list_alert_rules, list_incidents, list_on_call_schedules, list_escalation_policies, list_notification_connectors, list_teams
tags: [knowledge, onboarding]
examples:
  - Set up the knowledge for our team
  - Write the organization's knowledge from what is configured here
---

# Initialize knowledge

Goal: a knowledge base that answers "what runs here, how do we watch it, what do we do when it breaks, and how do we get in" - written with the person, one confirmed file at a time, with an entry file MOLE.md that points to the rest.

Keep a task list with `todo_write` through the whole session: the scan, each interview topic, and the final MOLE.md.

## 1. Agree where to write

- A team chat writes its team's knowledge. A personal chat writes the organization's knowledge, which needs the agent.manage permission. Say which one this chat writes and ask the person to confirm with `ask_user` before anything else. If they wanted the other one, tell them to start a chat for that team (or a personal chat) and stop.
- Read what already exists with `list_knowledge` and `read_knowledge` (MOLE.md first). Build on it; never drop content the person did not ask to drop.

## 2. Scan what is set up

Look, without changing anything:

- Data: `list_streams` (names, types) and, for the main ones, `get_stream_schema`.
- Services: `list_apm_services` and `get_service_topology` for the call graph.
- Alerting: `list_alert_rules`, `list_incidents` for the last 30 days (which rules fire, which services they hit), `list_escalation_policies`, `list_on_call_schedules`, `list_notification_connectors`.
- For a team, narrow everything to the team: its schedules and escalation policies, the rules that page it, and the services those rules and incidents name. Use `list_teams` to find the team's members.

Then sum it up for the person in one paragraph: the main services and how they call each other, where the logs and traces are, which alerts fire most, and who is on call.

## 3. Interview topic by topic

Go through these topics in order. For each, ask what you could not find (at most three questions at a time, with choices when there are likely answers), then propose the file:

1. Services and topology: what each main service does, its owners, its dependencies and the critical paths.
2. Observability: which streams, dashboards and queries to start from for each service; what "normal" looks like.
3. Runbooks: for the alerts that fire most, what to check first and the safe first actions.
4. Common failures: past incidents and their causes, with how they were found.
5. Access: how to reach clusters, consoles and dashboards (never write secrets, passwords or tokens - only where to find them).

Before writing a topic, list the file path and a short summary of what it will hold, and wait for the person to agree. Then write it with `edit_knowledge` (they confirm the change once more in the chat). Use paths such as `services/payments.md`, `runbooks/high-error-rate.md`, `access.md`.

## 4. Write MOLE.md

Last, write or update MOLE.md: one line on what this knowledge covers, then a short table of contents with one line per file, each written as `@path` - for example `@runbooks/high-error-rate.md - what to check when the API error rate alert fires`. Keep MOLE.md under 80 lines; the details belong in the other files.

End with what was written and what is still missing.
