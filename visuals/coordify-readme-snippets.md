# Coordify README Visual Snippets

---

## Hero Banner

```markdown
<div align="center">
  <img src="assets/coordify-banner.svg" alt="Coordify" width="100%"/>
</div>
```

---

## Badges

Paste directly below the banner. Monochromatic style matching the dark aesthetic.

```markdown
<div align="center">

[![MIT license](https://img.shields.io/badge/license-MIT-111111?style=flat-square)](LICENSE)
[![Claude Code](https://img.shields.io/badge/claude-code-111111?style=flat-square)](https://claude.ai/code)
[![CAP Protocol](https://img.shields.io/badge/protocol-CAP-111111?style=flat-square)]()
[![local-first](https://img.shields.io/badge/local--first-✓-111111?style=flat-square)]()
[![version](https://img.shields.io/github/v/release/YOUR_USERNAME/coordify?style=flat-square&color=111111&label=release)](https://github.com/YOUR_USERNAME/coordify/releases)
[![stars](https://img.shields.io/github/stars/YOUR_USERNAME/coordify?style=flat-square&color=111111&label=stars)](https://github.com/YOUR_USERNAME/coordify/stargazers)

</div>
```

Replace `YOUR_USERNAME` with your GitHub username.

---

## Network Visualization (product screenshot)

```markdown
<div align="center">
  <img src="assets/coordify-network.svg" alt="Coordify live network — 5 agents, heat-scored edges" width="100%"/>
</div>
```

Use after the "How it works" section. This is the product screenshot.

---

## Session Replay

```markdown
<div align="center">
  <img src="assets/coordify-session-replay.svg" alt="Coordify heat history — conflict detected and resolved" width="100%"/>
</div>
```

Use in a "Heat History" or "Session Intelligence" section.

---

## Heat Scale

```markdown
<div align="center">
  <img src="assets/coordify-heat-scale.svg" alt="Coordify heat bands — Safe, Monitor, Overlap, Conflict" width="100%"/>
</div>
```

Use inline inside the heat scoring explanation section.

---

## Suggested README asset structure

```
coordify/
  assets/
    coordify-banner.svg
    coordify-network.svg
    coordify-session-replay.svg
    coordify-heat-scale.svg
  README.md
```

---

## Suggested README section order

```
1. Banner image
2. One-liner + badges
3. The problem (2–3 sentences)
4. Heat scale visual
5. "How it works" (CAP, states, heat formula summary)
6. Network visualization (product screenshot)
7. Session replay visual (heat history)
8. Install
9. CLI commands table
10. Architecture / file layout
11. Contributing
```

---

## Tagline options

```
Coordify helps terminal-based Claude Code agents know what each other are doing,
predict where they may collide, and coordinate before they damage the codebase.

———

Know. Predict. Coordinate.

———

Multi-agent coordination for Claude Code.
Your agents. One codebase. No collisions.
```

---

## CLI quickstart block (README code block)

```bash
# Install
npm install -g coordify

# Start in your project root — Coordify Core launches automatically
cd my-project
coordify init

# Open as many Claude Code terminals as you need
# Coordify handles the rest

# Live status
coordify status

# Heat between all agent pairs
coordify heat

# Full session stats
coordify stats
```
