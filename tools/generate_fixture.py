#!/usr/bin/env python3
"""Deterministic fixture generator for rockpool vertical slice."""
import json, os, textwrap, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__))) if "__file__" in dir() else "/home/mark/rockpool"
FIX = os.path.join("/home/mark/rockpool", "fixtures")
SRC = os.path.join(FIX, "sources")
os.makedirs(SRC, exist_ok=True)

nodes = []
edges = []
sources = {}  # source_id -> (document_id, text)

def N(id, kind, label, description=None, aliases=None, evidence=None):
    nodes.append({"id": id, "kind": kind, "label": label,
                  "description": description, "aliases": aliases or [],
                  "evidence": evidence or []})

def E(frm, to, kind, evidence=None, confidence=None):
    edges.append({"from": frm, "to": to, "kind": kind,
                  "evidence": evidence or [], "confidence": confidence})

def ev(doc, src, quote):
    return {"document_id": doc, "source_id": src, "start": None, "end": None, "quote": quote}

def add_source(src, doc, text):
    sources[src] = (doc, text)

# --- Documents & sections for renewal pricing core ---
# Doc 14: renewal discount
add_source("document-14-section-4-2", "document-14",
"""# doc: document-14
## Section 4.2 — Evergreen subscription fee adjustments

Holders of an active annual plan receive a loyalty consideration at term
renewal. The adjustment equals ten percent of the recurring charge when
payment is received within the grace interval. This benefit is governed by
the Renewal Discount Policy and applies automatically unless the account
is past due.
""")
add_source("document-14-section-4-3", "document-14",
"""# doc: document-14
## Section 4.3 — Exclusions

The loyalty consideration does not combine with introductory offers.
Accounts converted from trial workspaces follow the standard tariff.
""")
# Doc 07: cancellation / refund (distractor, high pagerank target links here)
add_source("document-07-section-2-1", "document-07",
"""# doc: document-07
## Section 2.1 — Standard tariff and cancellation

The standard tariff schedule applies to all monthly plans. Cancellation
takes effect at period end. Pricing policy documentation is maintained
by the billing operations team.
""")
# Doc 22: data retention (split evidence pair)
add_source("document-22-section-1-1", "document-22",
"""# doc: document-22
## Section 1.1 — Retention window

Workspace artifacts persist for ninety days after plan expiry. Restoration
requires an active subscription of any tier.
""")
add_source("document-22-section-1-2", "document-22",
"""# doc: document-22
## Section 1.2 — Restoration fee

Reactivation after the retention window incurs a one-time restoration fee
equal to one month of the prevailing tariff. Renewal pricing inquiries
should reference this clause alongside the retention window.
""")
# Doc 31: SLA uptime
add_source("document-31-section-3-1", "document-31",
"""# doc: document-31
## Section 3.1 — Uptime commitment

Production tenants receive a 99.9% monthly uptime commitment. Credits are
calculated as a proportion of the recurring charge.
""")
# Doc 40: security audit (weak lexical overlap topic: query says "audit logging", source says "chronicle of access events")
add_source("document-40-section-5-1", "document-40",
"""# doc: document-40
## Section 5.1 — Chronicle of access events

Each control-plane mutation emits an append-only chronicle entry recording
actor identity, timestamp, and affected resource. Entries are immutable for
thirteen months.
""")
# Doc 41: SSO
add_source("document-41-section-2-2", "document-41",
"""# doc: document-41
## Section 2.2 — Federated identity

Workforce authentication proceeds via federated identity providers
supporting SAML 2.0. Session lifetime is eight hours.
""")
# Doc 55: data export
add_source("document-55-section-6-1", "document-55",
"""# doc: document-55
## Section 6.1 — Egress of tenant artifacts

Bulk egress of tenant artifacts is available to administrators. Exports
are checksummed and delivered within one business day.
""")
# Doc 60: handbook (high-PageRank decoy)
add_source("document-60-handbook", "document-60",
"""# doc: document-60
## Company handbook — general

Welcome to the company. This handbook covers values, holidays, expenses,
and the standard tariff schedule reference. All teams link here.
""")
# filler sources
for i in [71, 72, 73, 74]:
    add_source(f"document-{i}-note", f"document-{i}",
f"""# doc: document-{i}
## Note

Operational note {i} for the billing operations team. Routine procedures
and references to the standard tariff schedule.
""")

# --- Core graph nodes ---
N("renewal-pricing", "concept", "Renewal pricing",
  "How recurring charges are set when a subscription term renews.",
  ["renewal price", "RDP", "evergreen pricing"])
N("renewal-discount-policy", "policy", "Renewal discount policy",
  "Loyalty consideration applied at term renewal for annual plans.",
  ["RDP", "loyalty discount"],
  [ev("document-14", "document-14-section-4-2", "loyalty consideration at term")])
N("generic-pricing-policy", "policy", "Generic pricing policy",
  "Standard tariff schedule for monthly plans and general billing operations.",
  ["standard pricing"],
  [ev("document-07", "document-07-section-2-1", "standard tariff schedule")])
N("document-14", "document", "Document 14 — Billing terms",
  "Billing terms governing renewals and discounts.", ["billing terms"])
N("document-14-section-4-2", "section", "Document 14 §4.2",
  "Evergreen subscription fee adjustments section.", ["4.2"],
  [ev("document-14", "document-14-section-4-2", "loyalty consideration at term")])
N("document-14-section-4-3", "section", "Document 14 §4.3",
  "Exclusions for loyalty consideration.", [],
  [ev("document-14", "document-14-section-4-3", "does not combine with introductory")])
N("document-07", "document", "Document 07 — Tariff schedule", "Monthly tariff text.", [])
N("document-07-section-2-1", "section", "Document 07 §2.1", "Standard tariff section.", [],
  [ev("document-07", "document-07-section-2-1", "standard tariff schedule")])
N("company-handbook", "document", "Company handbook",
  "General company reference. Irrelevant to most queries but heavily linked.", ["handbook"],
  [ev("document-60", "document-60-handbook", "Welcome to the company")])
N("billing-operations", "entity", "Billing operations team",
  "Team owning tariff documentation.", ["billing team"])
# cycle: renewal-pricing -> annual-plan -> grace-interval -> renewal-pricing
N("annual-plan", "concept", "Annual plan", "Twelve-month subscription term.", ["annual"])
N("grace-interval", "concept", "Grace interval", "Payment window after term end.", ["grace period"])
N("past-due-account", "entity", "Past due account", "Account failing payment.", [])
# data retention split-evidence topic
N("retention-window", "concept", "Retention window",
  "Ninety-day persistence of artifacts after plan expiry.", ["retention"],
  [ev("document-22", "document-22-section-1-1", "ninety days")])
N("restoration-fee", "concept", "Restoration fee",
  "One-time fee for reactivation after retention window.", ["reactivation fee"],
  [ev("document-22", "document-22-section-1-2", "restoration fee")])
N("document-22", "document", "Document 22 — Retention", "Retention policy doc.", [])
N("document-22-section-1-1", "section", "Document 22 §1.1", "Retention window text.", [],
  [ev("document-22", "document-22-section-1-1", "ninety days")])
N("document-22-section-1-2", "section", "Document 22 §1.2", "Restoration fee text.", [],
  [ev("document-22", "document-22-section-1-2", "restoration fee")])
# SLA topic (two-hop: sla-concept -> uptime-claim -> section)
N("sla-uptime", "concept", "SLA uptime", "Production uptime commitment.", ["uptime", "SLA"])
N("uptime-credit-claim", "claim", "Uptime credit claim",
  "Credits are a proportion of recurring charge for missed SLA.", ["credits"],
  [ev("document-31", "document-31-section-3-1", "Credits are")])
N("document-31-section-3-1", "section", "Document 31 §3.1", "Uptime commitment text.", [],
  [ev("document-31", "document-31-section-3-1", "99.9% monthly uptime")])
# audit logging (weak lexical overlap: query "audit logging" vs "chronicle of access events")
N("audit-logging", "concept", "Audit logging",
  "Append-only chronicle of control-plane mutations with actor identity.", ["audit trail", "access chronicle"],
  [ev("document-40", "document-40-section-5-1", "append-only chronicle")])
N("document-40-section-5-1", "section", "Document 40 §5.1", "Chronicle of access events.", [],
  [ev("document-40", "document-40-section-5-1", "append-only chronicle")])
# SSO topic
N("sso-login", "concept", "SSO login",
  "Federated identity via SAML providers.", ["single sign-on", "SSO"],
  [ev("document-41", "document-41-section-2-2", "federated identity")])
N("document-41-section-2-2", "section", "Document 41 §2.2", "Federated identity text.", [],
  [ev("document-41", "document-41-section-2-2", "federated identity")])
# export topic
N("tenant-export", "concept", "Tenant export",
  "Bulk egress of tenant artifacts for administrators.", ["data export", "bulk download"],
  [ev("document-55", "document-55-section-6-1", "Bulk egress")])
N("document-55-section-6-1", "section", "Document 55 §6.1", "Egress text.", [],
  [ev("document-55", "document-55-section-6-1", "Bulk egress")])
# dead ends
for i in range(1, 6):
    N(f"dead-end-{i}", "concept", f"Dead end topic {i}",
      "Unconnected topic with no evidence and no outgoing edges.", [f"deadend{i}"])
# duplicate alias entity
N("regional-discount-program", "policy", "Regional discount program",
  "Unrelated regional pricing initiative sharing the RDP alias.", ["RDP"],
  [ev("document-07", "document-07-section-2-1", "standard tariff schedule")])

# filler concepts to reach ~70 nodes
fillers = [
    ("invoice-schedule", "concept", "Invoice schedule", "Monthly invoice cadence."),
    ("trial-workspace", "concept", "Trial workspace", "Evaluation workspace before purchase."),
    ("introductory-offer", "concept", "Introductory offer", "First-term promotional pricing."),
    ("monthly-tariff", "concept", "Monthly tariff", "Per-month list price."),
    ("expense-policy", "concept", "Expense policy", "Employee expense rules."),
    ("holiday-calendar", "concept", "Holiday calendar", "Company holidays."),
    ("saml-provider", "entity", "SAML provider", "External identity provider."),
    ("session-lifetime", "concept", "Session lifetime", "Eight-hour session duration."),
    ("checksum-export", "concept", "Checksum export", "Integrity verification for exports."),
    ("workspace-artifact", "entity", "Workspace artifact", "Stored user data object."),
    ("plan-expiry", "event", "Plan expiry", "Subscription term end event."),
    ("reactivation", "event", "Reactivation", "Subscription restart event."),
    ("billing-dispute", "event", "Billing dispute", "Charge disagreement event."),
    ("credit-note", "entity", "Credit note", "Billing credit instrument."),
    ("recurring-charge", "concept", "Recurring charge", "Periodic subscription fee."),
    ("control-plane", "entity", "Control plane", "Management API surface."),
    ("actor-identity", "entity", "Actor identity", "Authenticated principal."),
    ("retention-claim", "claim", "Retention claim", "Artifacts persist ninety days."),
    ("export-claim", "claim", "Export claim", "Exports delivered within one business day."),
    ("sso-claim", "claim", "SSO claim", "Workforce auth via federated identity."),
    ("discount-exclusion-claim", "claim", "Discount exclusion claim", "Loyalty consideration does not combine with introductory offers."),
    ("annual-plan-claim", "claim", "Annual plan claim", "Annual plans receive loyalty consideration."),
    ("doc-71-note", "section", "Note 71", "Operational note."),
    ("doc-72-note", "section", "Note 72", "Operational note."),
    ("doc-73-note", "section", "Note 73", "Operational note."),
    ("doc-74-note", "section", "Note 74", "Operational note."),
    ("values-page", "section", "Values page", "Company values section."),
    ("expenses-page", "section", "Expenses page", "Expense guidance section."),
]
for fid, kind, label, desc in fillers:
    N(fid, kind, label, desc, [])

# --- Edges (core + decoys + cycles + hub links) ---
E("renewal-pricing", "renewal-discount-policy", "related_to", confidence=0.9)
E("renewal-pricing", "generic-pricing-policy", "related_to", confidence=0.4)
E("renewal-pricing", "annual-plan", "related_to")
E("annual-plan", "grace-interval", "related_to")
E("grace-interval", "renewal-pricing", "related_to")  # cycle
E("renewal-discount-policy", "document-14-section-4-2", "supports",
  [ev("document-14", "document-14-section-4-2", "loyalty consideration at term")], 0.95)
E("document-14", "document-14-section-4-2", "contains")
E("document-14", "document-14-section-4-3", "contains")
E("renewal-discount-policy", "discount-exclusion-claim", "supports")
E("discount-exclusion-claim", "document-14-section-4-3", "supports")
E("generic-pricing-policy", "document-07-section-2-1", "supports")
E("document-07", "document-07-section-2-1", "contains")
E("renewal-pricing", "past-due-account", "mentions")
E("annual-plan", "annual-plan-claim", "supports")
E("annual-plan-claim", "document-14-section-4-2", "supports")
E("retention-window", "restoration-fee", "related_to")
E("retention-window", "document-22-section-1-1", "supports")
E("restoration-fee", "document-22-section-1-2", "supports")
E("document-22", "document-22-section-1-1", "contains")
E("document-22", "document-22-section-1-2", "contains")
E("plan-expiry", "retention-window", "refers_to")
E("reactivation", "restoration-fee", "refers_to")
E("retention-claim", "document-22-section-1-1", "supports")
E("sla-uptime", "uptime-credit-claim", "supports")
E("uptime-credit-claim", "document-31-section-3-1", "supports")
E("audit-logging", "document-40-section-5-1", "supports")
E("control-plane", "audit-logging", "mentions")
E("actor-identity", "audit-logging", "mentions")
E("sso-login", "document-41-section-2-2", "supports")
E("sso-claim", "document-41-section-2-2", "supports")
E("saml-provider", "sso-login", "refers_to")
E("session-lifetime", "sso-login", "part_of")
E("tenant-export", "document-55-section-6-1", "supports")
E("export-claim", "document-55-section-6-1", "supports")
E("checksum-export", "tenant-export", "part_of")
E("workspace-artifact", "retention-window", "refers_to")
E("workspace-artifact", "tenant-export", "refers_to")
E("recurring-charge", "renewal-pricing", "refers_to")
E("monthly-tariff", "generic-pricing-policy", "refers_to")
E("invoice-schedule", "monthly-tariff", "related_to")
E("trial-workspace", "introductory-offer", "related_to")
E("introductory-offer", "discount-exclusion-claim", "contradicts")
E("regional-discount-program", "document-07-section-2-1", "supports")
E("billing-dispute", "credit-note", "related_to")
E("credit-note", "uptime-credit-claim", "related_to")
# High-PageRank hub: many nodes link to handbook + generic policy
hub_targets = ["company-handbook", "generic-pricing-policy", "billing-operations"]
hub_sources = ["invoice-schedule", "trial-workspace", "introductory-offer", "monthly-tariff",
               "expense-policy", "holiday-calendar", "saml-provider", "session-lifetime",
               "checksum-export", "workspace-artifact", "plan-expiry", "reactivation",
               "billing-dispute", "credit-note", "recurring-charge", "control-plane",
               "actor-identity", "retention-claim", "export-claim", "sso-claim",
               "values-page", "expenses-page", "doc-71-note", "doc-72-note",
               "doc-73-note", "doc-74-note", "annual-plan", "grace-interval",
               "sla-uptime", "tenant-export", "sso-login", "audit-logging"]
for s in hub_sources:
    E(s, "company-handbook", "refers_to")
for s in hub_sources[:20]:
    E(s, "generic-pricing-policy", "mentions")
E("billing-operations", "generic-pricing-policy", "mentions")
E("billing-operations", "company-handbook", "mentions")
E("document-14", "billing-operations", "mentions")
E("company-handbook", "expense-policy", "contains")
E("company-handbook", "holiday-calendar", "contains")

print(f"nodes={len(nodes)} edges={len(edges)} sources={len(sources)}")
with open(os.path.join(FIX, "graph.json"), "w") as f:
    json.dump({"nodes": nodes, "edges": edges}, f, indent=2)
for src, (doc, text) in sources.items():
    with open(os.path.join(SRC, src + ".md"), "w") as f:
        f.write(text)

# --- Eval cases ---
def case(id, query, entities, evidence, hops=3):
    return {"id": id, "query": query, "expected_entities": entities,
            "expected_evidence": evidence, "max_hops": hops}

dev = [
    case("q01", "What policies affect renewal pricing?",
         ["renewal-discount-policy"], ["document-14#document-14-section-4-2"], 2),
    case("q02", "How is the loyalty discount applied at term renewal?",
         ["renewal-discount-policy"], ["document-14#document-14-section-4-2"], 3),
    case("q03", "Does the renewal discount combine with introductory offers?",
         ["discount-exclusion-claim"], ["document-14#document-14-section-4-3"], 3),
    case("q04", "How long do workspace artifacts persist after plan expiry?",
         ["retention-window"], ["document-22#document-22-section-1-1"], 2),
    case("q05", "What fee applies when reactivating after the retention window?",
         ["restoration-fee", "retention-window"], [
             "document-22#document-22-section-1-1",
             "document-22#document-22-section-1-2"], 3),
    case("q06", "What uptime commitment applies to production tenants?",
         ["sla-uptime", "uptime-credit-claim"], ["document-31#document-31-section-3-1"], 3),
    case("q07", "How are uptime credits calculated?",
         ["uptime-credit-claim"], ["document-31#document-31-section-3-1"], 2),
    case("q08", "What records exist for control-plane mutations?",  # weak lexical overlap
         ["audit-logging"], ["document-40#document-40-section-5-1"], 2),
    case("q09", "How long are access event records kept?",  # weak lexical overlap
         ["audit-logging"], ["document-40#document-40-section-5-1"], 2),
    case("q10", "How does workforce authentication work?",
         ["sso-login"], ["document-41#document-41-section-2-2"], 2),
    case("q11", "How long do SSO sessions last?",
         ["sso-login", "session-lifetime"], ["document-41#document-41-section-2-2"], 3),
    case("q12", "How can administrators get tenant artifacts out?",
         ["tenant-export"], ["document-55#document-55-section-6-1"], 2),
    case("q13", "Are bulk exports checksummed and how fast are they delivered?",
         ["export-claim", "tenant-export"], ["document-55#document-55-section-6-1"], 3),
    case("q14", "What happens to past due accounts at renewal?",
         ["past-due-account", "renewal-discount-policy"], ["document-14#document-14-section-4-2"], 3),
    case("q15", "What is the grace period for renewal payment?",
         ["grace-interval", "annual-plan"], ["document-14#document-14-section-4-2"], 3),
    case("q16", "Which plans receive the loyalty consideration?",
         ["annual-plan-claim", "annual-plan"], ["document-14#document-14-section-4-2"], 3),
    case("q17", "What does the standard tariff schedule say about monthly plans?",
         ["generic-pricing-policy"], ["document-07#document-07-section-2-1"], 2),
    case("q18", "Can trial workspaces use the renewal discount?",
         ["trial-workspace", "discount-exclusion-claim"], ["document-14#document-14-section-4-3"], 3),
    case("q19", "What triggers the retention window?",
         ["plan-expiry", "retention-window"], ["document-22#document-22-section-1-1"], 2),
    case("q20", "How is reactivation priced relative to tariff?",
         ["reactivation", "restoration-fee"], ["document-22#document-22-section-1-2"], 3),
    case("q21", "What identity proof supports audit entries?",  # weak lexical
         ["actor-identity", "audit-logging"], ["document-40#document-40-section-5-1"], 3),
    case("q22", "Which providers can be used for federated login?",
         ["saml-provider", "sso-login"], ["document-41#document-41-section-2-2"], 3),
    case("q23", "What integrity guarantees apply to bulk egress?",
         ["checksum-export", "tenant-export"], ["document-55#document-55-section-6-1"], 3),
    case("q24", "How are billing disputes credited?",
         ["billing-dispute", "credit-note"], ["document-31#document-31-section-3-1"], 3),
    case("q25", "What is the recurring charge basis for credits?",
         ["recurring-charge", "uptime-credit-claim"], ["document-31#document-31-section-3-1"], 3),
    case("q26", "Tell me about flibbertigibbet quantum widgets?",  # negative: no entity
         [], [], 2),
    case("q27", "What is the renewal policy on Mars colonies?",  # negative: no supported path
         [], [], 2),
    case("q28", "Dead end topic 3 details?",  # dead-end: entity but no evidence
         ["dead-end-3"], [], 2),
    case("q29", "RDP renewal terms?",  # ambiguous alias: two RDP nodes
         ["renewal-discount-policy"], ["document-14#document-14-section-4-2"], 2),
    case("q30", "Annual plan renewal and grace interval?",
         ["annual-plan", "grace-interval"], ["document-14#document-14-section-4-2"], 3),
]
holdout = [
    case("h01", "What loyalty benefit applies when an annual term renews?",
         ["renewal-discount-policy"], ["document-14#document-14-section-4-2"], 2),
    case("q_dummy_never_inspect", "placeholder", [], [], 2),
    case("h02", "After expiry, how soon must we act before paying a restoration fee?",
         ["retention-window", "restoration-fee"], [
             "document-22#document-22-section-1-1",
             "document-22#document-22-section-1-2"], 3),
    case("h03", "Which immutable records track who changed what and when?",  # weak lexical
         ["audit-logging"], ["document-40#document-40-section-5-1"], 2),
    case("h04", "What single sign-on method is available to the workforce?",
         ["sso-login"], ["document-41#document-41-section-2-2"], 2),
    case("h05", "How quickly are administrator bulk downloads fulfilled?",
         ["tenant-export"], ["document-55#document-55-section-6-1"], 2),
    case("h06", "Do past-due balances forfeit the evergreen adjustment?",
         ["past-due-account"], ["document-14#document-14-section-4-2"], 3),
    case("h07", "Zzxqxv nominal nonsense query with no answer?", [], [], 2),
    case("h08", "What monthly list price applies per the tariff?",
         ["monthly-tariff", "generic-pricing-policy"], ["document-07#document-07-section-2-1"], 3),
]
# remove placeholder
holdout = [c for c in holdout if c["id"] != "q_dummy_never_inspect"]
with open(os.path.join(FIX, "cases_dev.json"), "w") as f:
    json.dump(dev, f, indent=2)
with open(os.path.join(FIX, "cases_holdout.json"), "w") as f:
    json.dump(holdout, f, indent=2)
print(f"dev={len(dev)} holdout={len(holdout)}")
