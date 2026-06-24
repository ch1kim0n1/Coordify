---
name: Feature request
about: Suggest something for Coordify
title: "[feat] "
labels: enhancement
body:
  - type: markdown
    attributes:
      value: |
        Coordify is intentionally local-first, zero-outbound, and minimal in
        dependencies. Features that add telemetry, cloud, accounts, or a heavy
        dependency tree are out of scope for the foreseeable future. See the
        [Roadmap](../README.md#roadmap) for what is already planned.
  - type: textarea
    id: problem
    attributes:
      label: What problem does this solve?
      description: The use case, not the solution. Who hits this, when, and what do they do today?
    validations:
      required: true
  - type: textarea
    id: proposal
    attributes:
      label: Proposed solution
      description: What should Coordify do instead? Optional — a clear problem statement is enough to start discussion.
  - type: dropdown
    id: component
    attributes:
      label: Component
      options:
        - coordify-core (Rust)
        - coordify-hook (Node)
        - coordify-cli (TS)
        - coordify-sim (TS)
        - documentation
        - other
    validations:
      required: true
  - type: checkboxes
    id: scope
    attributes:
      label: Scope check
      options:
        - label: This does not require outbound network calls, telemetry, or an account
          required: true
        - label: I checked the Roadmap and this is not already listed
          required: true
