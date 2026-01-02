---
description: Audit app layout for Android/Material Design 3 compliance
---

# Layout Audit

Audit the app layout against Android best practices, Material Design 3 guidelines, and accessibility requirements.

## Instructions

Read the android-layout-audit skill first:
- `.claude/skills/android-layout-audit/SKILL.md`
- `.claude/skills/android-layout-audit/CHECKLIST.md`

Then perform a systematic audit of the layout:

### 1. Gather Layout Files

Find all relevant files:
```bash
fd -e svelte -e vue -e html -e css -e scss
```

### 2. Check Touch Targets

Search for small interactive elements:
```bash
# Find button/interactive styles
rg "\..*btn|button" --type css -A 5

# Check for min-height/min-width
rg "min-height|min-width" --type css

# Find onclick handlers
rg "onclick|on:click|@click" --type svelte --type vue --type html
```

### 3. Check Spacing System

Find non-8dp spacing values:
```bash
rg ":\s*(5|7|9|10|11|13|14|15|17|18|19|21|22|23|25|26|27)px" --type css
rg ":\s*0\.(3125|4375|5625|6875|8125|9375)rem" --type css
```

### 4. Check Edge-to-Edge Support

Verify safe area handling:
```bash
rg "safe-area-inset" --type css
rg "env\(safe-area" --type css
```

### 5. Check Viewport Meta

```bash
rg "viewport" index.html
```

### 6. Check Accessibility

```bash
# Images without alt
rg "<img" --type html --type svelte | head -20

# SVG icons (often missing labels)
rg "<svg" --type svelte -l

# Buttons without text
rg "<button[^>]*>" --type svelte -A 1
```

### 7. Check Color Contrast

Identify color pairs to verify:
- Text colors vs background colors
- Status indicator colors vs background
- Disabled state colors

Use a contrast checker tool for verification.

## Output Format

Provide a structured report:

```markdown
## Layout Audit Report

### Summary
- **Critical Issues**: X
- **High Priority**: X
- **Medium Priority**: X
- **Low Priority**: X

### Critical Issues
[List issues that must be fixed]

### High Priority Issues
[List important issues]

### Medium Priority Issues
[List nice-to-have fixes]

### Recommendations
[Specific suggestions with code examples]
```

## CleanScope-Specific Notes

- Video container uses 4:3 aspect ratio (1920×1440 endoscope)
- Debug controls are power-user features (smaller targets acceptable)
- Dark theme optimized for medical/inspection use
- Status bar shows real-time streaming info

## Related Skills

- `android-layout-audit` - Full audit guidelines
- `accessibility-plugin:ux-implementation` - For implementing fixes
