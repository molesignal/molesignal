---
name: insight
description: Look back at how Mole Agent was used in this chat's scope - what people asked, how the answers and automations went, what was rated unhelpful and why, which tools failed - and write a one-page review with what to keep, what to fix and what knowledge to add. Read only.
argument-hint: [days to look back, default 30]
version: "1"
allowed-tools: get_agent_insights, list_knowledge, read_knowledge, list_skills, todo_write
tags: [review, insights]
examples:
  - How did Mole Agent do for our team this month?
  - Review the last 7 days of automations
---

# Insight

Goal: a one-page review of how Mole Agent served the people in this chat's scope, ending with changes worth making. A team chat looks at its team; a personal chat looks at the person and the teams they belong to. Change nothing: this is a report.

## 1. Read the numbers

- Call `get_agent_insights`. If the person gave a number of days, pass it as `days`; otherwise leave it out (30 days).
- Read the entry file MOLE.md and the list of knowledge files with `list_knowledge` and `read_knowledge`, and the skills with `list_skills`, so the suggestions build on what exists.

## 2. Write the review

Keep it to one page, in this order:

1. **In short** - two or three sentences: how much Mole Agent was used and whether it helped.
2. **What went well** - answers rated helpful, automations that ran cleanly, tools that worked.
3. **Friction** - answers that failed or were stopped, automations that failed, were blocked (a question, a confirmation or a missing credential) or skipped, the reasons people gave for unhelpful answers, and tools that failed often. Name each one and say how often.
4. **Changes worth making** - at most five, most useful first. Each is concrete and says where it goes:
   - a knowledge file to add or extend (its path and what to write in it), when answers lacked context about services, access or runbooks;
   - a skill to write, when the same kind of question keeps coming back;
   - an automation to fix: a credential to save, a prompt to tighten, a schedule to change;
   - a tool that keeps failing, and what it needs (a permission, a data source, a setting).

When there is little data - a few questions or none - say so plainly and keep the review short instead of guessing.
