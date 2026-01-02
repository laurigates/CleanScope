---
created: 2025-12-30
modified: 2025-12-30
reviewed: 2025-12-30
name: android-layout-audit
description: Audit Android app layouts for Material Design 3 compliance, accessibility, edge-to-edge support, and modern best practices. Use for layout reviews, pre-release checks, or establishing baseline quality.
allowed-tools: Bash, Read, Grep, Glob, TodoWrite, Task, WebSearch
---

# Android Layout Audit

Expert knowledge for auditing Android app layouts against Material Design 3 guidelines, accessibility requirements, and modern Android platform conventions.

## Audit Categories

### 1. Touch Target Size (Accessibility)

**Requirement**: All interactive elements must be at least 48dp × 48dp.

**Check CSS/Styles for:**
```css
/* Buttons/interactive elements should be >= 48px (48dp on mdpi) */
min-height: 48px;
min-width: 48px;
padding: /* sum with content must reach 48dp */
```

**Common Issues:**
- Buttons with only padding, no min-height/min-width
- Icon buttons without sufficient touch area
- Close/dismiss buttons that are too small

**Patterns to Search:**
```bash
# Find button/interactive element styles
rg -i "button|btn|clickable|tap|touch" --type css
rg "min-height|min-width|padding" --type css

# For web-based apps (Tauri/Capacitor)
rg "onclick|@click|on:click" --type svelte --type vue --type html
```

### 2. Color Contrast (Accessibility)

**Requirements:**
| Content Type | Minimum Ratio |
|--------------|---------------|
| Body text (< 18pt or < 14pt bold) | 4.5:1 |
| Large text (≥ 18pt or ≥ 14pt bold) | 3:1 |
| UI components & graphics | 3:1 |

**Common Colors to Check:**
```css
/* Text on dark backgrounds */
color: #888;  /* May be too low contrast on #000 */
color: #666;  /* Likely fails on #1a1a1a */

/* Status indicators */
color: #4ade80;  /* Green - check against background */
color: #fbbf24;  /* Yellow - often fails contrast */
color: #ef4444;  /* Red - usually okay */
```

**Tools:**
- [WebAIM Contrast Checker](https://webaim.org/resources/contrastchecker/)
- Chrome DevTools Accessibility panel
- Accessibility Scanner app (Android)

### 3. Edge-to-Edge Display (Android 15+)

**Requirement**: Apps targeting SDK 35+ must handle edge-to-edge display.

**CSS Properties to Use:**
```css
/* Safe area insets for notches, navigation bars */
padding-bottom: env(safe-area-inset-bottom);
padding-top: env(safe-area-inset-top);
padding-left: env(safe-area-inset-left);
padding-right: env(safe-area-inset-right);

/* Or combined */
padding: env(safe-area-inset-top) env(safe-area-inset-right)
         env(safe-area-inset-bottom) env(safe-area-inset-left);
```

**Common Issues:**
- Content hidden behind navigation bar
- Interactive elements in gesture navigation zones
- Status bar overlapping content

**Patterns to Search:**
```bash
# Check for safe-area usage
rg "safe-area-inset" --type css
rg "env\(safe-area"

# Check viewport meta
rg "viewport-fit=cover" --type html
```

### 4. Spacing System (8dp Grid)

**Requirement**: Use multiples of 8dp for consistent spacing.

**Valid Values:**
- 4dp (half-unit, use sparingly)
- 8dp, 16dp, 24dp, 32dp, 40dp, 48dp, 56dp, 64dp

**Invalid Values to Flag:**
```css
/* Non-standard spacing */
padding: 5px;   /* Should be 4px or 8px */
margin: 12px;   /* Should be 8px or 16px */
gap: 10px;      /* Should be 8px or 12px */
padding: 15px;  /* Should be 16px */
```

**Patterns to Search:**
```bash
# Find non-8dp values
rg ":\s*(5|7|9|10|11|13|14|15|17|18|19|21|22|23|25|26|27)px" --type css
rg ":\s*(5|7|9|10|11|13|14|15|17|18|19|21|22|23|25|26|27)dp" --type css
```

### 5. Typography Scale

**Material Design 3 Type Scale:**
| Style | Size | Line Height | Weight |
|-------|------|-------------|--------|
| Display Large | 57sp | 64sp | 400 |
| Display Medium | 45sp | 52sp | 400 |
| Display Small | 36sp | 44sp | 400 |
| Headline Large | 32sp | 40sp | 400 |
| Headline Medium | 28sp | 36sp | 400 |
| Headline Small | 24sp | 32sp | 400 |
| Title Large | 22sp | 28sp | 400 |
| Title Medium | 16sp | 24sp | 500 |
| Title Small | 14sp | 20sp | 500 |
| Body Large | 16sp | 24sp | 400 |
| Body Medium | 14sp | 20sp | 400 |
| Body Small | 12sp | 16sp | 400 |
| Label Large | 14sp | 20sp | 500 |
| Label Medium | 12sp | 16sp | 500 |
| Label Small | 11sp | 16sp | 500 |

**Minimum Readable Size**: 12sp (never smaller for body text)

### 6. Animation Timing

**Material Design Motion:**
| Duration | Use Case |
|----------|----------|
| 100ms | Minimum (avoid, feels abrupt) |
| 200ms | Standard micro-interactions |
| 300ms | Standard transitions |
| 400ms | Complex animations |
| 500ms+ | Large surface changes |

**Recommended Easing:**
```css
/* Standard easing (default) */
transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);

/* Deceleration (entering elements) */
transition-timing-function: cubic-bezier(0, 0, 0.2, 1);

/* Acceleration (exiting elements) */
transition-timing-function: cubic-bezier(0.4, 0, 1, 1);
```

### 7. Responsive/Adaptive Layout

**Window Size Classes:**
| Class | Width | Typical Devices |
|-------|-------|-----------------|
| Compact | < 600dp | Phones portrait |
| Medium | 600-839dp | Tablets portrait, foldables |
| Expanded | ≥ 840dp | Tablets landscape, desktop |

**Navigation Patterns by Size:**
| Size Class | Navigation Pattern |
|------------|-------------------|
| Compact | Bottom navigation bar |
| Medium | Navigation rail |
| Expanded | Navigation drawer |

### 8. Content Descriptions (A11y)

**Requirements:**
- All non-decorative images need `alt` or `aria-label`
- Interactive elements need accessible names
- Don't include element type in description ("Submit" not "Submit button")
- Unique descriptions for list items

**Patterns to Search:**
```bash
# Images without alt text
rg "<img[^>]*(?!alt=)" --type html --type svelte --type vue
rg "src=\"[^\"]+\"[^>]*>" --type html  # Then check for alt

# SVG icons (common source of issues)
rg "<svg" --type svelte --type vue --type html

# Buttons - should have text content or aria-label
rg "<button[^>]*>" --type html --type svelte --type vue
```

### 9. Viewport Configuration

**Required Meta Tag:**
```html
<meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">
```

**Key Properties:**
- `width=device-width` - Responsive width
- `initial-scale=1.0` - No initial zoom
- `viewport-fit=cover` - Edge-to-edge support
- `user-scalable=yes` - Accessibility (don't disable zoom)

### 10. Aspect Ratio Handling

**Common Camera Aspect Ratios:**
| Ratio | Resolution Examples | Use Case |
|-------|---------------------|----------|
| 4:3 | 1920×1440, 640×480 | Endoscopes, webcams |
| 16:9 | 1920×1080, 1280×720 | HD video |
| 1:1 | 1080×1080 | Square format |

**CSS for Aspect Ratio:**
```css
/* Modern approach */
aspect-ratio: 4 / 3;

/* Fallback for older browsers */
.container {
  position: relative;
  padding-bottom: 75%; /* 3/4 = 0.75 = 75% for 4:3 */
}
.container > * {
  position: absolute;
  inset: 0;
}
```

## Audit Checklist

### Critical (Must Fix)
- [ ] Touch targets ≥ 48dp × 48dp
- [ ] Text contrast ≥ 4.5:1 (body) / 3:1 (large)
- [ ] Edge-to-edge safe areas handled
- [ ] All interactive elements accessible

### High Priority
- [ ] 8dp grid spacing system
- [ ] Typography follows Material scale
- [ ] Animations 200-400ms with proper easing
- [ ] Viewport meta tag correct

### Medium Priority
- [ ] Responsive breakpoints defined
- [ ] Navigation adapts to screen size
- [ ] Aspect ratios handled for media content
- [ ] Color not sole indicator of state

### Low Priority
- [ ] Elevation/shadow hierarchy consistent
- [ ] Focus states visible
- [ ] Loading states present
- [ ] Error states styled consistently

## CleanScope-Specific Considerations

### Video Container
- **Aspect Ratio**: Endoscope outputs 1920×1440 (4:3)
- Container should maintain aspect ratio without letterboxing
- `object-fit: contain` for video/canvas scaling

### Debug Controls
- Touch targets may be smaller for power-user controls
- Consider collapsible/hidden state for casual users
- Ensure adequate spacing when visible

### Status Bar
- Must not overlap system status bar
- Consider always-on-display scenarios
- High contrast for outdoor/bright environments

## Parallel Audit Strategy

Launch multiple agents for comprehensive analysis:

1. **CSS Analysis Agent** - Check spacing, sizing, colors
2. **HTML/Template Agent** - Check accessibility attributes
3. **Viewport Agent** - Check meta tags, safe areas
4. **Component Agent** - Check interactive element sizing

## Integration

This skill works with:
- `/check:layout` command for quick audits
- `accessibility-plugin:ux-implementation` agent for fixes
- `code-review` agent for comprehensive review

## Resources

- [Material Design 3](https://m3.material.io/)
- [Android Accessibility](https://developer.android.com/guide/topics/ui/accessibility)
- [Edge-to-Edge Guide](https://developer.android.com/develop/ui/views/layout/edge-to-edge)
- [Window Size Classes](https://developer.android.com/develop/ui/compose/layouts/adaptive)
