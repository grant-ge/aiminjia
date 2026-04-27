## Step 3: Iterate Based on Feedback

The user has tested the skill and returned with feedback. Your job: fix issues quickly.

### Common Issues and Fixes

| User Says | What to Fix |
|-----------|-------------|
| "It didn't understand my file format" | Update SKILL.md input requirements + add column mapping guidance |
| "The calculation was wrong" | Fix scripts/*.py or business rules in SKILL.md |
| "It skipped a step" | Make the workflow instructions in SKILL.md more explicit |
| "It didn't ask me to confirm before [action]" | Add the tool to `confirm_before` in SKILL.md frontmatter |
| "The output format is wrong" | Update the Output section in SKILL.md |
| "It couldn't connect to DingTalk" | Check if dingtalk_* tools are in the `tools` frontmatter |

### Iteration Workflow

1. Listen to the user's feedback
2. Read the relevant file with `skill_smith_read_file`
3. Fix with `skill_smith_write_file`
4. Call `skill_smith_validate` to verify
5. Call `skill_smith_install` to update (overwrite existing)
6. Tell the user to test again in a new conversation

### When Done

When the user is satisfied, offer to export: "Would you like me to package this skill as a `.aijia-skill` file so you can share it?"
