---
name: Bug report
about: Something in Coordify does not behave as documented
title: "[bug] "
labels: bug
body:
  - type: markdown
    attributes:
      value: |
        Thanks for taking the time to file a bug. For security vulnerabilities,
        do **not** use this template — see [SECURITY.md](../SECURITY.md).
  - type: input
    id: version
    attributes:
      label: Coordify version
      description: Output of `coordify-core --version` and `coordify --version`
      placeholder: "coordify-core 0.1.0, coordify 0.1.0"
    validations:
      required: true
  - type: input
    id: platform
    attributes:
      label: Platform
      description: OS + version
      placeholder: "macOS 14.5 / Ubuntu 24.04"
    validations:
      required: true
  - type: input
    id: node-rust
    attributes:
      label: Toolchain
      description: Node and Rust versions
      placeholder: "node v20.11.0, rustc 1.78.0"
    validations:
      required: true
  - type: textarea
    id: what-happened
    attributes:
      label: What happened?
      description: Steps to reproduce, expected vs actual behavior
    validations:
      required: true
  - type: textarea
    id: logs
    attributes:
      label: Relevant logs
      description: Output of `coordify logs`, `coordify status`, or `.coordify/sessions/<id>/diagnostics.log`. Redact anything sensitive.
      render: shell
  - type: textarea
    id: scenario
    attributes:
      label: Reproduces with a sim scenario?
      description: If yes, attach a `scenarios/*.json` fixture. Bug fixes that ship with a failing fixture get merged faster.
  - type: checkboxes
    id: checks
    attributes:
      label: Checklist
      options:
        - label: I checked the [README](../README.md) and this is not intended behavior
          required: true
        - label: I checked existing issues for duplicates
          required: true
