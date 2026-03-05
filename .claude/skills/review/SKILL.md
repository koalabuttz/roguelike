---
name: review
description: Analyze code review feedback, verifying each point against actual code before responding.
---

# Code Review Analysis Skill

Systematically analyze code review feedback, verifying each point against the actual codebase before accepting or pushing back.

## Usage

- `/review` — Analyze review comments (user will paste or link them)
- `/review <PR URL>` — Fetch and analyze review comments from a GitHub PR

## Instructions

1. **Collect the review points.** If the user pastes comments, parse them. If given a PR URL, fetch comments with:

```bash
gh api repos/OWNER/REPO/pulls/NUMBER/comments
gh api repos/OWNER/REPO/pulls/NUMBER/reviews
```

2. **For EACH review point, do all of the following before responding:**

   a. **Read the actual code** being criticized — not just the diff, but the full file context.

   b. **Grep for all usages** of the function/type/pattern in question across the entire workspace, including:
      - Both micro and standard tiers
      - Code behind feature flags (`#[cfg(feature = "...")]`)
      - Test code (`#[cfg(test)]`)
      - The C64 crate (`crates/c64/`)

   c. **Classify the point** as one of:
      - **Valid bug** — genuine correctness issue, needs fix
      - **Valid improvement** — not a bug but would improve code quality
      - **Style-only** — subjective preference, not worth changing
      - **Invalid** — reviewer is wrong, explain why with evidence

   d. **For valid points**, draft the fix.
   e. **For invalid points**, write a response explaining why with code evidence (file paths, line numbers, grep results).

3. **Output a summary table:**

```
| # | Point | Classification | Action |
|---|-------|---------------|--------|
| 1 | "unused variable" | Invalid — used in cfg(test) | Respond with evidence |
| 2 | "missing error handling" | Valid bug | Fix drafted |
```

4. **NEVER agree with a review point without verifying it first.** The most common error is accepting a "dead code" claim without grepping for all references, including conditional compilation.

5. After the analysis, ask the user which valid points to implement and which invalid points need a response drafted.
