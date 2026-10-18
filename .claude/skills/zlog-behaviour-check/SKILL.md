---
name: zlog-behaviour-check
description: Check that zLog in Rust (zlog-rs) behaves like zLog. Use after implementing or changing a feature of zlog-rs, or when asked to verify behaviour, find bugs or compare with zLog. An independent subagent compares the Rust code with the zLog manual and Delphi source; every suspected bug is then investigated by a second, separate subagent before it is reported.
---

# zLog behaviour check

Two stages, each done by subagents that did not write the code under test.
The agent that wrote the code only orchestrates and reports.

## Inputs

- The feature or area to check (for example "ACAG scoring", "entry row keys", "ZLOX save").
- Rust code: this repository (crates under `crates/`).
- zLog source: a checkout of https://github.com/jr8ppg/zLog, directory `zlog/` (Delphi; many files are Shift_JIS, read with `iconv -f sjis -t utf8 -c FILE`).
- zLog manual: https://use.zlog.org/manual (table of contents) and its pages.

## Stage 1: verification

Start one subagent per area (several in parallel when the areas are independent).
Give it the area, the three sources above, and these instructions:

1. Read the manual pages for the area and the Delphi code that implements it; write down the expected behaviour as numbered, testable statements, each with its source (`file:line` or manual URL).
2. Read the Rust code for the area and decide for each statement: matches, differs, or not implemented.
3. Where possible, confirm by running code: add a throwaway test under the scratchpad or run `cargo test`; never edit files in the repository.
4. Report a table: statement, source, Rust location, verdict, evidence. List differences separately with a one-line failure scenario each.
5. Do not judge intent: a difference from zLog is reported even if it looks like an improvement; mark those "intentional?" when a comment in the Rust code documents the departure.

## Stage 2: investigation of each difference

For every difference from stage 1, start a new subagent that has not seen stage 1's reasoning, only the claim.
Give it the single claim (statement, failure scenario, locations) and these instructions:

1. Try to refute the claim first: re-read the zLog source and manual independently; check whether the Delphi behaviour really is as claimed, including the paths that call it.
2. Reproduce in Rust with a minimal test (scratchpad only).
3. If confirmed, find the root cause in the Rust code and propose the smallest fix, without applying it.
4. Verdict: CONFIRMED (bug), INTENTIONAL (documented departure), REFUTED (stage 1 was wrong), or UNCERTAIN, with evidence.

## Report

To the user, in their language: confirmed bugs first (scenario, cause, proposed fix), then intentional departures, then refuted and uncertain items in one line each.
Fix confirmed bugs only after reporting, or when the user has asked for fixes; rerun the check on the fixed area afterwards.
