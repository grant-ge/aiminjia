## Step 2: Install and Test

### Pre-install Checks

1. Call `skill_smith_dry_run` to verify the skill is loadable
2. If any check fails, read the failing file with `skill_smith_read_file`, fix it, and re-run

### Installation

1. Call `skill_smith_install` to install the skill
2. If conflict (skill ID already exists), ask the user whether to overwrite
3. On success, tell the user:

"Your skill **[name]** is now installed! To test it:
1. Start a new conversation (click + in sidebar)
2. Type: [trigger_text]
3. Follow the skill's workflow with real data

Come back here and tell me how it went — I can adjust anything that needs improvement."

### Export (Optional)

If the user wants to share the skill:
1. Call `skill_smith_export` to create a `.aijia-skill` package
2. Tell the user where the file was saved

### When to Proceed

Wait for the user to test and come back with feedback. Then advance to Step 3 (iteration).
