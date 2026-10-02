#!/usr/bin/env python3
"""Corpus B: harbor pilotage domain. Disjoint vocabulary from corpus A.

Design rules (H1/H5 isolation):
- Queries share vocabulary with node labels/aliases (seedable) but NOT with
  source passage wording (BM25-starved paraphrase).
- Multi-hop cases: the seed node carries NO evidence; the answer evidence
  lives 2 hops away and is unreachable from seed-node metadata alone.
- One high-fan-out node (25 outgoing edges) so max_frontier_size truncation
  bites: graph-prior-ranked truncation (hybrid) vs uniform (semantic-only).
"""
import json, os

FIX = "/home/mark/rockpool/fixtures/corpus_b"
SRC = os.path.join(FIX, "sources")
os.makedirs(SRC, exist_ok=True)

nodes, edges, sources = [], [], {}

def N(id, kind, label, description=None, aliases=None, evidence=None):
    nodes.append({"id": id, "kind": kind, "label": label,
                  "description": description, "aliases": aliases or [],
                  "evidence": evidence or []})

def E(frm, to, kind, evidence=None, confidence=None):
    edges.append({"from": frm, "to": to, "kind": kind,
                  "evidence": evidence or [], "confidence": confidence})

def ev(doc, src, quote):
    return {"document_id": doc, "source_id": src, "start": None,
            "end": None, "quote": quote}

def add_source(src, doc, text):
    sources[src] = (doc, text)

# ---------------- sources (deliberately paraphrased vs queries) ----------------
add_source("pilotage-ordinance-s3", "pilotage-ordinance",
"""# doc: pilotage-ordinance
## Section 3 — Compulsory pilotage waters

Every vessel exceeding five hundred gross tonnage shall embark a licensed
pilot before crossing the outer bar inbound. Masters must signal readiness
by hoisting the identification pennant at the foretop.
""")
add_source("buoyage-manual-wreck", "buoyage-manual",
"""# doc: buoyage-manual
## Wreck-marking buoys

A wreck-marking buoy exhibits two vertical red spheres by day. By night it
shows an occulting white illumination. Mariners shall give such marks a wide
berth until the obstruction is charted as cleared.
""")
add_source("fog-signals-s2", "fog-signals",
"""# doc: fog-signals
## Section 2 — Audible warnings in restricted visibility

Power-driven craft underway sound one prolonged blast at intervals not
exceeding two minutes. Sailing vessels add two short blasts thereafter.
""")
add_source("tide-tables-note", "tide-tables",
"""# doc: tide-tables
## Datum note

Soundings on this chart reduce to lowest astronomical tide. Dredged channels
maintain eight metres at datum; silting reports appear in the weekly notice.
""")
add_source("harbor-handbook-general", "harbor-handbook",
"""# doc: harbor-handbook
## General information

Welcome to the harbor. This handbook describes office hours, visitor mooring
fees, and the gift shop. All harbor publications reference this page.
""")
add_source("wreck-register-7", "wreck-register",
"""# doc: wreck-register
## Entry 7 — Obstruction register

The coaster Meridian rests upright in twenty-one metres, position logged
with the hydrographic office. A temporary mark was laid pending survey.
""")

# ---------------- core graph ----------------
# Seeds carry NO evidence; evidence lives 2 hops away.
N("large-vessel-entry", "concept", "Large vessel harbor entry",
  "How big ships come into port.", ["incoming ships", "harbor entry"])
N("compulsory-pilotage", "policy", "Compulsory pilotage rule",
  "Licensed pilot required past the outer bar.", ["pilot requirement"],
  [ev("pilotage-ordinance", "pilotage-ordinance-s3", "embark a licensed")])
N("pilotage-ordinance-s3", "section", "Pilotage ordinance §3",
  "Compulsory pilotage waters text.", [],
  [ev("pilotage-ordinance", "pilotage-ordinance-s3", "embark a licensed")])

N("sunken-hazard-marking", "concept", "Sunken hazard marking",
  "How drowned obstructions are signed for mariners.", ["sunken hazards"],
  [])  # NO evidence on purpose
N("wreck-buoy-protocol", "policy", "Wreck buoy protocol",
  "Red spheres by day, occulting light by night.", ["wreck marking"],
  [ev("buoyage-manual", "buoyage-manual-wreck", "two vertical red spheres")])
N("buoyage-manual-wreck", "section", "Buoyage manual (wreck marks)",
  "Wreck-marking buoy specification.", [],
  [ev("buoyage-manual", "buoyage-manual-wreck", "two vertical red spheres")])

N("low-visibility-sailing", "concept", "Low visibility sailing",
  "Moving in thick weather.", ["fog sailing", "restricted visibility"])
N("sound-signaling-rule", "policy", "Sound signaling rule",
  "Blast patterns when visibility fails.", ["foghorn rule"],
  [ev("fog-signals", "fog-signals-s2", "one prolonged blast")])
N("fog-signals-s2", "section", "Fog signals §2",
  "Audible warnings text.", [],
  [ev("fog-signals", "fog-signals-s2", "one prolonged blast")])

N("channel-depth", "concept", "Channel depth",
  "How deep the dredged way is.", ["draft clearance"])
N("datum-claim", "claim", "Datum depth claim",
  "Eight metres at lowest astronomical tide.", ["dredged depth"],
  [ev("tide-tables", "tide-tables-note", "eight metres at datum")])
N("tide-tables-note", "section", "Tide tables datum note",
  "Soundings datum text.", [],
  [ev("tide-tables", "tide-tables-note", "eight metres at datum")])

N("meridian-wreck", "entity", "Coaster Meridian",
  "Wreck in twenty-one metres.", ["meridian"],
  [ev("wreck-register", "wreck-register-7", "rests upright")])
N("wreck-register-7", "section", "Wreck register entry 7",
  "Obstruction register text.", [],
  [ev("wreck-register", "wreck-register-7", "rests upright")])

N("harbor-handbook", "document", "Harbor handbook",
  "General harbor reference; heavily linked but rarely relevant.", ["handbook"],
  [ev("harbor-handbook", "harbor-handbook-general", "Welcome to the harbor")])

E("large-vessel-entry", "compulsory-pilotage", "related_to", confidence=0.9)
E("compulsory-pilotage", "pilotage-ordinance-s3", "supports",
  [ev("pilotage-ordinance", "pilotage-ordinance-s3", "embark a licensed")], 0.95)
E("sunken-hazard-marking", "wreck-buoy-protocol", "related_to", confidence=0.9)
E("wreck-buoy-protocol", "buoyage-manual-wreck", "supports",
  [ev("buoyage-manual", "buoyage-manual-wreck", "two vertical red spheres")], 0.95)
E("wreck-buoy-protocol", "meridian-wreck", "mentions")
E("meridian-wreck", "wreck-register-7", "supports")
E("low-visibility-sailing", "sound-signaling-rule", "related_to", confidence=0.9)
E("sound-signaling-rule", "fog-signals-s2", "supports",
  [ev("fog-signals", "fog-signals-s2", "one prolonged blast")], 0.95)
E("channel-depth", "datum-claim", "supports")
E("datum-claim", "tide-tables-note", "supports")

# ---------------- high-fan-out node for H5 ----------------
# harbor-chart points at 25 markers; only wreck-buoy-protocol (and the
# ornament meridian link) lead anywhere relevant to the wreck-marking query.
N("harbor-chart", "concept", "Harbor chart markers",
  "Index of charted marks and hazards.", ["chart index"])
E("sunken-hazard-marking", "harbor-chart", "related_to")
E("harbor-chart", "wreck-buoy-protocol", "supports",
  [ev("buoyage-manual", "buoyage-manual-wreck", "two vertical red spheres")], 0.9)
for i in range(1, 25):
    mid = f"marker-{i:02d}"
    N(mid, "concept", f"Chart marker {i}",
      "Routine charted mark with no bearing on wreck inquiries.", [f"marker{i}"])
    E("harbor-chart", mid, "mentions")
    E(mid, "harbor-handbook", "refers_to")
E("harbor-chart", "harbor-handbook", "refers_to")
for mid in ["large-vessel-entry", "low-visibility-sailing", "channel-depth"]:
    E(mid, "harbor-handbook", "refers_to")

print(f"nodes={len(nodes)} edges={len(edges)} sources={len(sources)}")
with open(os.path.join(FIX, "graph.json"), "w") as f:
    json.dump({"nodes": nodes, "edges": edges}, f, indent=2)
for src, (doc, text) in sources.items():
    with open(os.path.join(SRC, src + ".md"), "w") as f:
        f.write(text)

def case(id, query, entities, evidence, hops=3):
    return {"id": id, "query": query, "expected_entities": entities,
            "expected_evidence": evidence, "max_hops": hops}

cases = [
    # Multi-hop, seed has NO evidence (H1 isolation).
    case("b01", "Where must large vessels embark a harbor pilot?",
         ["compulsory-pilotage"], ["pilotage-ordinance#pilotage-ordinance-s3"], 2),
    case("b02", "How are sunken hazards marked for navigation?",
         ["wreck-buoy-protocol"], ["buoyage-manual#buoyage-manual-wreck"], 3),
    case("b03", "What sound patterns apply when sailing in thick weather?",
         ["sound-signaling-rule"], ["fog-signals#fog-signals-s2"], 2),
    case("b04", "How deep is the dredged way at datum?",
         ["datum-claim"], ["tide-tables#tide-tables-note"], 2),
    case("b05", "Where does the coaster Meridian rest?",
         ["meridian-wreck"], ["wreck-register#wreck-register-7"], 3),
    # High-fan-out selection: relevant protocol among 24 decoy markers.
    case("b06", "Which charted mark protocol covers sunken hazards?",
         ["wreck-buoy-protocol"], ["buoyage-manual#buoyage-manual-wreck"], 3),
    # Split evidence across two sources.
    case("b07", "What marks the Meridian wreck and what does the general wreck protocol require?",
         ["meridian-wreck", "wreck-buoy-protocol"], [
             "wreck-register#wreck-register-7",
             "buoyage-manual#buoyage-manual-wreck"], 3),
    # Negative: nonsense.
    case("b08", "Quantum ballast teleportation procedures?", [], [], 2),
    # Ambiguous-ish: handbook decoy shares vocabulary with everything.
    case("b09", "What are the harbor office hours?",
         ["harbor-handbook"], ["harbor-handbook#harbor-handbook-general"], 2),
    # Two-hop with paraphrased detail (seedable via policy wording).
    case("b10", "What must be hoisted under the compulsory pilotage rule?",
         ["compulsory-pilotage"], ["pilotage-ordinance#pilotage-ordinance-s3"], 3),
]
with open(os.path.join(FIX, "cases.json"), "w") as f:
    json.dump(cases, f, indent=2)
print(f"cases={len(cases)}")
