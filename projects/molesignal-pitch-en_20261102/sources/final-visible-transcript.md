# 01_cover

MoleSignal
INVESTOR DECK · NOV 2026
Observability meetsincident response
Open source. Self-hosted. Your choice of AI model.
Raising $750,000 for a 24-month plan
zek.carter · Founder | From product delivery to customer validation
Pre-launch refinement · No external users or paying customers · Company not yet incorporated
01 / 14

# 02_pain

01 / CUSTOMER PAIN
More tools. Still too much operational work.
Fragmented context
Logs, metrics, traces andincidents span workflows.Engineers repeatedly aligntime, services, ownershipand response progress.
Cost: investigation and handoffs
Complex total cost
SaaS bills grow acrossmultiple usage dimensions.Self-hosting shifts work tocomponent operations,storage and engineering.
Cost: budget and platform effort
More control needs
Telemetry can be sensitive.AI adds model access,tool permissions andapprovals to the samegovernance boundary.
Cost: governance and deployment
Our hypothesis: teams want signals, response and control in one product.
Target-customer hypotheses, pending interviews and pilots; not claims about every tool or team.
02 / 14

# 03_solution

02 / PRODUCT APPROACH
Investigate and respond in one environment
Unified telemetry
Logs, metrics, traces and cross-signal context.SQL / PromQL for investigation.
Incident response
Alerts, on-call, escalation and status updates.Carry findings into response and review.
Mole Agent
Gather evidence within platform permissions.Choose models; approve high-risk actions.
Self-managed deployment
Rust binary, PostgreSQL metadata,local or object storage; standard protocols.
Start with existing alerts. Migrate telemetry when the team is ready.
Based on current code and documentation. Pre-launch; no external production validation yet.
03 / 14

# 04_logs

02 / PRODUCT VIEW · LOGS
Find the events behind a time-window spike
Log explorer
SQL and filtersTime and severityService / trace_id
Keep query contextinside the investigation
Actual product frontend · Test-fixture demo data, not customer data
04 / 14

# 05_traces

02 / PRODUCT VIEW · TRACES
Investigate latency and errors by request
Trace explorer
Service and statusDuration and errorsSpans / RED views
Connect requestswith log context
Actual product frontend · Test-fixture demo data · Investigation efficiency gains not yet measured
05 / 14

# 06_response

02 / INCIDENT RESPONSE & MOLE AGENT
Carry investigation into team response
For the on-call team
Receive, group and route incoming alerts.Define ownership with on-call and escalation.Record progress and publish status updates.Turn incident reviews into follow-up work.
For controlled AI investigation
Record hypotheses and supporting evidence.Respect platform identity and permissions.Choose models, including internal endpoints.Require approval for high-risk actions.
Proposed value: fewer tool handoffs. Reliability and outcomes still need pilot validation.
Current implementation and documentation; no guarantee of correct root cause or unapproved remediation.
06 / 14

# 07_customer

03 / TARGET CUSTOMERS
Start with teams short on platform capacity
Users
SREs, platform and backend engineersand on-call responders
Buyers
CTOs, engineering and platform leadersBuying managed operations and time savings
Initial customer hypothesis
Uses Prometheus, Grafana or cloud tools.Needs on-call without a large platform team.Wants less upkeep and control over data.Start with SaaS and internet-service teams.
Acquire through open source, founder outreach and design partners; start with alert response.
Customer profile and acquisition channels are hypotheses. Design-partner pipeline remains unvalidated.
07 / 14

# 08_competition

04 / COMPETITIVE LANDSCAPE
Customers already have strong alternatives
Datadog
Broad SaaS platform forobservability, response and AI.Multiple product andusage-based pricing units.
Fits teams prioritizing breadthand accepting that pricing model.
Grafana
Loki, Mimir and Tempo;Cloud IRM with AI investigation.Composable projects andmanaged cloud options.
Fits teams valuing flexibilityand existing ecosystem investment.
OpenObserve
Unified telemetry, self-hostingand Cloud; incidents and AI.AGPL core; Cloud primarilypriced per GB ingested.
Significant technical overlap;workflow fit must earn the choice.
AI, a single binary and object storage are not exclusive moats.
Official descriptions checked Nov 2, 2026. Fit assessments are judgments, not benchmark results. Sources in notes.
08 / 14

# 09_difference

04 / WHY MOLESIGNAL
For teams prioritizing control and response
Openness and control
Apache 2.0, self-hosted data and model choice.A fit when deployment control is essential.
One response experience
Investigation, on-call, status and approvals.Aim to reduce assembly and handoff effort.
Incremental adoption
Accept existing alerts and standard telemetry.Test one use case before broader migration.
Advantages to prove
Prove setup, response efficiency and retention.TCO and performance gains remain unproven.
Differentiation rests on workflow fit and execution. A durable moat must develop through real adoption.
09 / 14

# 10_business

05 / BUSINESS MODEL
Open-source adoption. Cloud subscriptions.
Open-source self-hosting
Free · Apache 2.0
Customers run their infrastructure and data.Build trust through code, docs and deployment.Learn from actual needs and product feedback.
Cloud subscriptions
Planned · Pricing TBD
Proposed: usage allowance plus overages.Hosting, upgrades, backups and support.Test willingness to pay before setting tiers.
Measure conversion, retention, and unit costs for storage, queries, models and support.
Cloud has not launched. Subscription structure is a proposal, not published pricing or a margin commitment.
10 / 14

# 11_stage

06 / CURRENT STAGE
Product built. Customer validation ahead.
Pre-launch
0 external users0 paying customers
Company not yet incorporatedCloud has not launched
Delivered so far
Backend, frontend, deployment and user docs.Telemetry, response and Agent implementation.
Next validation
External installs, workloads and recovery.Repeat use, willingness to pay, support effort.
Founder-provided stage as of Nov 2, 2026. Implemented features are not customer validation or commercial traction.
11 / 14

# 12_founder

07 / FOUNDER
zek.carter | Founder, MoleSignal
Production-systems experience
Years in backend / infrastructure; Rust and Go.Cloud-native platforms and financial trading.Designed and built matching, liquidation,clearing and settlement systems.
Low latency · State machines · Raft · WAL / SnapshotFailure recovery · Consistency of financial balances
End-to-end product delivery
Selected work: a decentralized-exchangematching engine; a clearing/settlement rewrite.Independently led MoleSignal product definition,architecture, technology choices and full-stackdevelopment, assisted by AI coding tools.
Founder fit: translating failure, recovery and consistency requirements into a usable product.
Founder-provided background. No employer names, exact tenure or unverified scale claims included.
12 / 14

# 13_raise

08 / FUNDING PLAN
$750,000 for 24 months of validation
$750,000
24-month planning horizon
Average budget: $31,250/month,including contingency. Actual spenddepends on hiring and cloud usage.
Proposed allocation · Subject to detailed budgeting
55% · $412,500   Core product and R&D
20% · $150,000   Cloud infrastructure and operations
15% · $112,500   Customer validation and marketing
10% · $75,000     Incorporation, legal and contingency
Priorities: reliability, operable Cloud, real demand and initial willingness to pay.
Raise and horizon supplied by founder; allocation is proposed, not current burn. Valuation and instrument not specified here.
13 / 14

# 14_milestones

08 / 24-MONTH VALIDATION PLAN
Prove value through adoption and payment
Months 0–6
Release and incorporateTarget 3–5 design partnersValidate setup and recovery
Gate: successful installsand continued use
Months 7–12
Launch invite-only Cloud pilotTarget 3 paying teamsTest tiers and support cost
Gate: demonstratedwillingness to pay
Months 13–24
Target 10 paying teamsObserve usage and renewalsTest unit costs and acquisition
Gate: retention andrepeatable delivery
MoleSignal · Let's walk through the product and technical diligence
molesignal.com   /   github.com/molesignal/molesignal
Proposed targets, not achieved results or commitments. Timeline starts when funding is received.
14 / 14