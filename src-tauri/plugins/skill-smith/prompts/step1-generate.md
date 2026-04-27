## Step 1: Generate the Skill

Generate ALL skill files in this single step. Do NOT split across multiple steps.

### File Generation Order

1. **plugin.toml** — Write first. Minimal metadata.
2. **SKILL.md** — The core file. Include:
   - YAML frontmatter (name, description, tools, confirm_before)
   - Detailed workflow instructions the AI will follow at runtime
   - Domain knowledge, business rules, examples
   - Confirmation checkpoints for dangerous operations
3. **scripts/*.py** (if needed) — Reusable Python for deterministic data processing:
   - Only create scripts for logic that MUST be identical every time (calculations, transformations)
   - The AI can write ad-hoc Python at runtime — scripts are for repeatability
4. **references/*.json** (if needed) — Static business data:
   - Price lists, field mappings, category tables, threshold rules
   - The AI loads these via execute_python at runtime

### SKILL.md Writing Guide

Write the SKILL.md body as if you're onboarding a smart colleague:

```markdown
# [Skill Name]

## When to Use
[Describe the trigger scenario clearly]

## Input Requirements
[What files/data the user needs to provide]

## Workflow

### 1. [First Step Name]
[Detailed instructions for what the AI should do]
[Which tool to use and how]
[What to show the user, what to ask]

### 2. [Second Step Name]
...

## Business Rules
[Domain-specific rules, thresholds, formulas]

## Output
[What the final deliverable looks like]
```

### After Generation

1. Call `skill_smith_validate` to check for errors
2. If errors exist, fix them and re-validate
3. Show the user a summary: "I've created [skill name] with [N] files. Here's what it does: [summary]"
4. Ask: "Shall I install it so you can test?"
