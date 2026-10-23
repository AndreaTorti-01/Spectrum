---
paths:
  - "**/*.md"
  - "site/**"
  - "src/**"
  - ".github/**"
  - "installer/**"
---

# Copy rules

These cover all human-readable text except code: UI strings (including the gamut-space descriptions in `color.rs`), README, website, issue and PR templates, `.claude/` docs, release notes, commit messages and PR descriptions. Identifiers and code comments are exempt.

Visuals carry the message and text captions them. The giveaway of machine-written copy is volume: text that explains what the reader can already see, restates the headline or describes benefits in the abstract.

## App UI strings

- Prefer sentence case and no period on labels or buttons. Buttons are usually verbs ("Read from monitor", "Restore recommended HDR settings"). Avoid "please", exclamation marks and "Oops": they add tone but no information.
- Status messages work best as a past-tense fact in about 6 words or fewer that names the thing: "Applied Display P3 to monitor 1". "Successfully" adds nothing.
- Errors usually say what failed and what to do, in 2 short sentences or fewer: "Couldn't read EDID from monitor 2. Reconnect the cable and try again."
- Hints fit best on a disabled or non-obvious control, about 8 words or fewer: "Turn on HDR to tune it".
- Gamut-space descriptions: usually 1 sentence of about 18 words or fewer, with a concrete fact (who uses it, what it covers).
- Units: "450 nits", "93%" (no space before %), "2560 × 1440". A middle dot " · " may separate items in info lines.

## Budgets

- Website: aim for about 380 words of prose in total. Per section, a headline of 7 words or fewer, a body of about 30 or fewer in one paragraph, and no body at all in a couple of sections. Hero paragraph about 28 words or fewer.
- README: aim for about 550 words above "Build from source" and no section over about 90 words of prose. Screenshot captions of 10 words or fewer, or none.

## Tests for every sentence

- Delete test: if removing a sentence loses nothing the headline, screenshot or demo gives, leave it out. That usually covers "As you can see", restated headlines, section intros or outros and closing summaries.
- Specificity: each body sentence has a concrete noun from this product (byte offset, file name, Windows setting, measured number, monitor model, menu label). "Better color", "more control" and "workflow" are too vague to earn a place.
- Truth: every number comes from a measurement. Every behavior claimed exists in the code. If unsure, cut it. Avoid "coming soon", roadmaps and invented users or quotes: readers can't verify them.

## Patterns to avoid

These read as machine-written, so avoid them unless there's a reason.

- Negative parallelism: "not X, but Y", "It's not just X, it's Y", "no X, no Y, just Z", "Y rather than X".
- Lists of exactly three adjectives or fragments ("Fast. Light. Free."). One, two or four items read more naturally.
- Em dashes, and spaced en dashes as clause glue. A period, comma, colon or parentheses does the same job. Ranges like 100–4000 are fine.
- Participle tails ("…, making it easy to…", "…, ensuring…", "…, allowing you to…").
- Copula dodges: serves as, stands as, features, offers, boasts, provides a way to. "Is" or "has" is plainer.
- Rhetorical-question openers, "Whether you're X or Y", "Meet Spectrum", "Say goodbye to", "Introducing", "The result?", colon reveals.
- Summary closers: "In short", "That's it.", "Simple as that."
- Hedges and hype adverbs: simply, just (as softener), easily, effortlessly, seamlessly, truly, actually (as filler), incredibly.
- Vocabulary: delve, crucial, pivotal, tapestry, testament, underscore, showcase, intricate, garner, enduring, leverage, harness, elevate, unlock, empower, streamline, robust, powerful, intuitive, comprehensive, cutting-edge, game-changer, revolutionize, supercharge, journey, landscape, realm, ever-evolving, "in today's world", "at its core", "designed to", "next-level".
- Vague attribution ("Many users…", "Experts…"). Elegant variation: "Spectrum", "the app" or "it" beats "this powerful tool".

## Formatting and voice

- Prefer sentence case for headings, no emoji, no bold lead-in bullets, bold at most once per README section, bullets for parallel items (steps, flags, FAQ), no horizontal rules between README sections, and up to 3 badges (CI, release, license).
- Plain, dry, confident. Second person or imperative, contractions fine. Write for an engineer who knows Windows, color profiles and HDR. Vary sentence length. One fragment per section is plenty.
- Avoid a fictional "we". Use first person only if the owner supplies a reason for the README.
- One joke per page is enough: the footer line "Built with Win32, GDI and a stubborn refusal to ship Electron."
- Name real things: "Windows HDR Calibration app", "Settings › Display › Color profile". Mention DisplayCAL or ArgyllCMS mainly to say Spectrum doesn't need them.

## Review

When reviewing, for each line apply the delete test, the specificity test and the banned scan, then count words against the budget. Grep for `—`, "successfully", "please", "!" and the vocabulary list. Output a table of line, violation and rewritten line (or DELETE), then the corrected text. Rewriting is more useful than flagging alone.
