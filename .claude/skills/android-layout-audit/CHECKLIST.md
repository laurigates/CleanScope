# Android Layout Audit Checklist

Quick-reference checklist for auditing Android app layouts.

## Accessibility

### Touch Targets
- [ ] All buttons/interactive elements ≥ 48dp × 48dp
- [ ] Adequate spacing between touch targets (≥ 8dp)
- [ ] No overlapping touch areas

### Color Contrast
- [ ] Body text: ≥ 4.5:1 contrast ratio
- [ ] Large text (≥ 18pt): ≥ 3:1 contrast ratio
- [ ] UI components: ≥ 3:1 contrast ratio
- [ ] Don't rely on color alone for meaning

### Screen Readers
- [ ] All images have alt text (or marked decorative)
- [ ] Interactive elements have accessible names
- [ ] Focus order is logical
- [ ] No duplicate content descriptions

## Edge-to-Edge (Android 15+)

### Safe Areas
- [ ] Content respects `safe-area-inset-top`
- [ ] Content respects `safe-area-inset-bottom`
- [ ] Content respects `safe-area-inset-left/right` (landscape)
- [ ] Interactive elements not in gesture zones

### System Bars
- [ ] Status bar doesn't overlap content
- [ ] Navigation bar doesn't obscure controls
- [ ] Correct handling of transparent system bars

## Material Design 3

### Spacing (8dp Grid)
- [ ] Padding uses 8dp multiples (4, 8, 16, 24, 32...)
- [ ] Margins use 8dp multiples
- [ ] Gaps use 8dp multiples
- [ ] No odd values like 5dp, 10dp, 15dp

### Typography
- [ ] Body text ≥ 14sp
- [ ] Minimum text size ≥ 11sp
- [ ] Consistent type scale used
- [ ] Line heights appropriate

### Animation
- [ ] Transitions 200-400ms
- [ ] No animations < 100ms
- [ ] Proper easing curves used
- [ ] Animations can be disabled (reduce motion)

## Responsive Layout

### Viewport
- [ ] `width=device-width` in viewport meta
- [ ] `initial-scale=1.0` set
- [ ] `viewport-fit=cover` for edge-to-edge
- [ ] Zoom not disabled (`user-scalable` not `no`)

### Breakpoints
- [ ] Compact (< 600dp): Phone layout
- [ ] Medium (600-839dp): Tablet/foldable layout
- [ ] Expanded (≥ 840dp): Large screen layout

### Navigation
- [ ] Bottom nav for compact
- [ ] Navigation rail for medium (optional)
- [ ] Drawer for expanded (optional)

## Media Handling

### Video/Camera (CleanScope-specific)
- [ ] 4:3 aspect ratio supported (1920×1440)
- [ ] `object-fit: contain` for scaling
- [ ] Canvas/video fills available space
- [ ] No letterboxing on matching aspect ratio

### Images
- [ ] Responsive images or max-width
- [ ] Lazy loading for lists
- [ ] Placeholder/loading states

## Component States

### Interactive Elements
- [ ] Default state styled
- [ ] Hover/focus state visible
- [ ] Pressed/active state visible
- [ ] Disabled state distinguishable

### Feedback
- [ ] Loading indicators present
- [ ] Error states styled
- [ ] Success feedback shown
- [ ] Empty states handled

## Quick Commands

```bash
# Find touch target issues (values < 48)
rg "min-height:\s*([0-3]?\d|4[0-7])px" --type css
rg "height:\s*([0-3]?\d|4[0-7])px" --type css

# Find non-8dp spacing
rg ":\s*(5|7|9|10|11|13|14|15|17|18|19|21|22|23|25|26|27)px" --type css

# Check for safe-area usage
rg "safe-area-inset" --type css

# Find images without alt
rg "<img[^>]+>" --type html | grep -v "alt="

# Check viewport meta
rg "viewport" index.html
```

## Severity Guide

| Issue | Severity | Impact |
|-------|----------|--------|
| Touch target < 48dp | Critical | Unusable for some users |
| Contrast < 3:1 | Critical | Text unreadable |
| Missing safe areas | High | Content obscured |
| Missing alt text | High | Screen reader issues |
| Non-8dp spacing | Medium | Visual inconsistency |
| Wrong animation timing | Low | Polish issue |
