# 01_cover

MoleSignal is an open-source, fully self-hostable observability and incident-response platform. It aims to keep investigation context, team response and model control in one environment. Founder zek.carter is seeking $750,000 for a 24-month plan to move from independent product delivery to early customer validation. The product is in pre-launch refinement, with no external users or paying customers, and the company has not yet been incorporated. The raise, horizon and stage are founder-provided facts.

---

# 02_pain

The target problem has three parts: fragmented investigation context, difficult total-cost management, and control over sensitive telemetry and AI access. Switching among signals and incident workflows can create investigation and handoff work. SaaS usage charges and self-hosted component operations create different cost structures. These are target-customer hypotheses, not findings from completed interviews or universal criticisms of every vendor. Early discovery must establish frequency, urgency and willingness to pay.

---

# 03_solution

Current code and documentation cover telemetry, incident response, Mole Agent and self-managed deployment. SQL, PromQL and cross-signal context support investigations across logs, metrics and traces. Alerts feed on-call, escalation, status updates and review. Mole Agent uses platform permissions, selectable models and approval for high-risk actions. Deployment uses a Rust binary, PostgreSQL metadata and local or object storage. Teams can begin with existing alerts and migrate telemetry later. These implementations have not yet been validated by external production customers. Evidence: project README and current route implementations, reviewed Nov 2, 2026.

---

# 04_logs

This is an actual frontend screenshot from the repository log explorer, with fields, time distribution, severity, service and trace identifiers. It uses Playwright fixture data, not customer telemetry. The visible counts and services are not evidence of production performance or commercial adoption. The product intent is to preserve query and time context for further investigation. Image source: web/playwright/tests/05-visual.spec.ts-snapshots/logs-light-comfortable-chromium-darwin.png. The corresponding test explicitly uses a mocked backend.

---

# 05_traces

The trace explorer organizes service, status, duration, errors and trace identifiers around individual requests, with entry points for spans, RED and service views. Together with logs it supports cross-signal investigation. All counts, latencies and service names shown are demonstration fixtures. No customer investigation-time improvement has been measured. Image source: web/playwright/tests/05-visual.spec.ts-snapshots/traces-light-comfortable-chromium-darwin.png; 05-visual.spec.ts explicitly uses a mocked backend.

---

# 06_response

Incident response includes alert intake, grouping, routing, on-call, escalation, status updates and review follow-ups. Mole Agent records hypotheses and evidence; it does not guarantee a correct root cause. Teams can select internal model endpoints, and tool execution uses platform identity and permissions, with high-risk operations subject to approval. Fewer handoffs is a proposed value, while reliability, time savings and investigation quality require pilot validation. Evidence: current README sections on incident response and Mole Agent.

---

# 07_customer

Users are SREs, platform and backend engineers, and on-call responders. Buyers are expected to be CTOs and engineering or platform budget owners. The initial hypothesis is SaaS and internet-service teams that already have monitoring but limited platform capacity. Payment should be justified by managed operations and saved effort, not feature counts alone. Planned acquisition begins with open source, founder outreach and design partners, using alert response as the first use case. This profile and partner pipeline remain unvalidated.

---

# 08_competition

Datadog combines observability, incident response and AI investigation, with multiple product and usage pricing dimensions. Grafana offers the Loki, Mimir and Tempo ecosystem, alongside Cloud IRM with on-call and AI investigation. OpenObserve offers unified telemetry, single-binary self-hosting and Cloud, as well as incidents, workflows and AI SRE. Its open-source core is AGPL and Cloud is primarily priced by GB ingested. Fit assessments are our interpretation of official descriptions, not hands-on competitor benchmarks. AI, Rust, object storage and a single binary are not exclusive advantages. Official sources checked Nov 2, 2026: https://www.datadoghq.com/product/incident-response/ ; https://www.datadoghq.com/product/ai/bits-investigation/ ; https://www.datadoghq.com/pricing/ ; https://grafana.com/oss/ ; https://grafana.com/products/cloud/irm/ ; https://openobserve.ai/platform/ ; https://openobserve.ai/pricing/ .

---

# 09_difference

The reason to consider MoleSignal is the combined fit of Apache 2.0, self-hosting, model choice, a unified response experience and incremental adoption. These are selection hypotheses, not proven superior outcomes. We must validate setup success, response efficiency, retention, total cost and performance under comparable conditions. OpenObserve in particular has substantial technical overlap. Existing Datadog or Grafana customers need a clear migration benefit. Evidence: MoleSignal LICENSE and README, plus the official competitor sources on the preceding slide.

---

# 10_business

The business model uses open-source self-hosting to build adoption and trust, and Cloud subscriptions to monetize managed operations. The source is Apache 2.0. Cloud is still on a waitlist and has not launched. A subscription with an included allowance and usage overages is a proposed structure, not published pricing. Hosting, upgrades, backups and support are planned service value. We must measure conversion, retention and the costs of storage, queries, models and support before claiming revenue or margins. Evidence: project LICENSE and website Cloud/pricing copy; the subscription structure is a planning proposal.

---

# 11_stage

The product has a backend, frontend, deployment and user documentation, and implementations spanning telemetry, response and the Agent. This demonstrates product delivery, not customer validation or traction. The founder confirms zero external users and zero paying customers, pre-launch refinement, and no incorporated company yet. Cloud has not launched. Next priorities are external installs, real workloads, recovery, continued use, willingness to pay and support burden. Stage supplied by the founder on Nov 2, 2026; implementation evidence is the current repository and README.

---

# 12_founder

zek.carter has years of backend and infrastructure experience, primarily using Rust and Go, across cloud-native platforms and financial trading systems. His responsibilities include matching, liquidation, clearing and settlement design and development, covering low latency, state machines, Raft availability, WAL and snapshots, recovery and consistency of financial balances. Selected deliveries include a decentralized-exchange matching engine and a clearing/settlement rewrite. He independently leads MoleSignal product definition, architecture, technology choices and full-stack development, assisted by AI coding tools. This is founder-provided background; no employer, exact tenure or unverified scale has been added.

---

# 13_raise

The founder plans to raise $750,000 for 24 months. The suggested allocation is 55%, or $412,500, to core product and R&D; 20%, or $150,000, to cloud infrastructure and operations; 15%, or $112,500, to customer validation and early marketing; and 10%, or $75,000, to incorporation, legal and contingency. This totals $750,000 and averages $31,250 monthly, including contingency; it is not current burn. Allocation remains a proposal and depends on staffing, location and cloud consumption. The investment entity and financing structure need to be established before execution. No valuation or instrument is specified in this deck.

---

# 14_milestones

The proposed clock begins when funding is received. In months zero to six, release the product, incorporate and target three to five design partners while validating installation and recovery. In months seven to twelve, launch an invite-only Cloud pilot and target three paying teams while testing tiers and support costs. In months thirteen to twenty-four, target ten paying teams and observe continued use, renewals, unit costs and acquisition. These are proposed targets, not achieved outcomes or commitments. If validation fails, refine the use case before expanding spending. We invite a product walkthrough and technical diligence. Product and source: https://molesignal.com ; https://github.com/molesignal/molesignal .