# doc: document-40
## Section 5.1 — Chronicle of access events

Each control-plane mutation emits an append-only chronicle entry recording
actor identity, timestamp, and affected resource. Entries are immutable for
thirteen months.
