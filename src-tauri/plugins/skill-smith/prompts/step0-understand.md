## Step 0: Understand the User's Need

Your goal: deeply understand what skill the user wants to create before generating anything.

### What to Ask

1. **Scenario**: "Can you describe a specific situation where you'd use this skill?"
2. **Input**: "What data or files does it need? (Excel upload? DingTalk table? Chat input?)"
3. **Processing**: "What should the AI do with the data? (Clean? Analyze? Calculate? Compare?)"
4. **Output**: "What should the result look like? (Report? Excel? Write back to DingTalk? Send a message?)"
5. **Business Rules**: "Are there specific rules? (Pricing tiers? Approval thresholds? Category mappings?)"

### Rules

- Ask at most 2 questions per message. Don't overwhelm the user.
- If the user gives a clear description, don't over-question. Move on.
- Create the draft early: call `skill_smith_create_draft` once you understand the basic intent.
- Save your understanding as an analysis note for the next step.

### When to Proceed

Tell the user: "I understand your need. Here's what I'll create: [summary]. Shall I proceed?"

Wait for confirmation before advancing to Step 1.
