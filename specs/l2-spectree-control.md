---
id: L2-SPECTREE-CONTROL-001
level: L2
title: Authoritative SpecTree control plane
status: LOCKED
parent: L1-PROJECT-001
code_targets:
  - .spectree/config.json
  - .gitignore
  - specs/**
  - changes/**
  - .agents/**
  - .baseline/**
  - .claude/**
  - .vscode/**
  - .env
  - coverage/**
  - test-results/**
test_targets:
  - package.json
---

# Authoritative SpecTree control plane

Every product file has a semantic owner. Targets use file paths or globs rather
than inert directory literals. Local tools, credentials, generated coverage,
test artifacts, and build manifests remain outside authorization baselines.
