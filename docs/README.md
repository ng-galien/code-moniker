# Documentation

This tree is organized by task. Start with the surface you want to use,
then drop into the reference pages only when you need exact grammar or SQL
details.

## CLI

| Need | Page |
| ---- | ---- |
| Dump a symbol graph for a file or directory | [Extract](cli/extract.md) |
| Measure extraction coverage and scan time | [Stats](cli/stats.md) |
| List declared deps with their package moniker | [Manifest](cli/manifest.md) |
| Run architecture and naming rules | [Check](cli/check.md) |
| Write rule expressions | [Rule DSL](cli/check-dsl.md) |
| Discover supported language tags, kinds, and shapes | [Discovery](cli/langs.md) |
| Connect `check` to hooks, pre-commit, or CI | [Agent harness](cli/agent-harness.md) |

## PostgreSQL

| Need | Page |
| ---- | ---- |
| Create tables, populate graphs, and query them | [Usage](postgres/usage.md) |
| Look up SQL types, functions, operators, and indexes | [Reference](postgres/reference.md) |
| Validate declarative graph JSON | [Declare schema](postgres/declare-schema.json) |

## Design

| Need | Page |
| ---- | ---- |
| Understand the model and extraction contract | [Spec](design/spec.md) |
| Understand moniker URI grammar and matching | [Moniker URI](design/moniker-uri.md) |

## Project

| Need | Page |
| ---- | ---- |
| Build, test, or add a language | [Contributing](../CONTRIBUTING.md) |
| Review benchmark numbers | [Performance](perf.md) |
