---
paths:
  - "**/*.md"
  - "site/**"
  - "src/**"
  - ".github/**"
  - "installer/**"
---

# Copy rules

These cover all human-readable text except code: UI strings (including the gamut-space descriptions in `color.rs`), README, website, CONTRIBUTING, issue and PR templates, `.claude/` docs, release notes, commit messages and PR descriptions. Identifiers and code comments are exempt.

Visuals carry the message and text captions them. The giveaway of machine-written copy is volume: text that explains what the reader can already see, restates the headline or describes benefits in the abstract.

## App UI strings

- Sentence case, no period on labels or buttons. Buttons are verbs ("Read profile from monitor", "Reset HDR"). No "please", no exclamation marks, no "Oops".
- Status messages are a past-tense fact in 6 words or fewer that names the thing: "Applied Display P3 to monitor 1". No "successfully".
- Errors say what failed and what to do, in at most 2 short sentences: "Couldn't read EDID from monitor 2. Reconnect the cable and try again."
- Hints appear only on a disabled or non-obvious control, 8 words or fewer: "Turn on HDR to tune it".
- Gamut-space descriptions: 1 sentence, 18 words or fewer, with a concrete fact (who uses it, what it covers).
- Units: "450 nits", "93%" (no space before %), "2560 × 1440". A middle dot " · " may separate items in info lines.

## Budgets

- Website: 380 words of prose in total. Per section: headline 7 words or fewer, body 30 or fewer, one paragraph at most, and no body at all in at least two sections. Hero paragraph 28 words or fewer.
- README: 550 words above "Build from source", no section over 90 words of prose. Screenshot captions 10 words or fewer, or none.

## Tests for every sentence

- Delete test: if removing it loses nothing the headline, screenshot or demo gives, it stays deleted. No "As you can see", no restated headlines, no section intros or outros, no closing summary.
- Specificity: each body sentence has a concrete noun from this product (byte offset, file name, Windows setting, measured number, monitor model, menu label). "Better color", "more control" and "workflow" are out.
- Truth: every number comes from a measurement. Every behavior claimed exists in the code. If unsure, cut it. No "coming soon", no roadmap, no invented users or quotes.

## Banned

- Negative parallelism: "not X, but Y", "It's not just X, it's Y", "no X, no Y, just Z", "Y rather than X".
- Lists of exactly three adjectives or fragments ("Fast. Light. Free."). Use one, two or four.
- Em dashes, and spaced en dashes as clause glue. Use a period, comma, colon or parentheses. Ranges like 100–4000 are fine.
- Participle tails ("…, making it easy to…", "…, ensuring…", "…, allowing you to…").
- Copula dodges: serves as, stands as, features, offers, boasts, provides a way to. Say "is" or "has".
- Rhetorical-question openers, "Whether you're X or Y", "Meet Spectrum", "Say goodbye to", "Introducing", "The result?", colon reveals.
- Summary closers: "In short", "That's it.", "Simple as that."
- Hedges and hype adverbs: simply, just (as softener), easily, effortlessly, seamlessly, truly, actually (as filler), incredibly.
- Vocabulary: delve, crucial, pivotal, tapestry, testament, underscore, showcase, intricate, garner, enduring, leverage, harness, elevate, unlock, empower, streamline, robust, powerful, intuitive, comprehensive, cutting-edge, game-changer, revolutionize, supercharge, journey, landscape, realm, ever-evolving, "in today's world", "at its core", "designed to", "next-level".
- Vague attribution ("Many users…", "Experts…"). Elegant variation: say "Spectrum", "the app" or "it", never "this powerful tool".

## Formatting and voice

- Sentence case for every heading. No emoji. No bold lead-in bullets. Bold at most once per README section. Bullets only for parallel items (steps, flags, FAQ). No horizontal rules between README sections. At most 3 badges (CI, release, license).
- Plain, dry, confident. Second person or imperative, contractions fine. Write for an engineer who knows Windows, color profiles and HDR. Vary sentence length. At most one fragment per section.
- No fictional "we". First person only if the owner supplies a reason for the README.
- One joke per page: the footer line "Built with Win32, GDI and a stubborn refusal to ship Electron."
- Name real things: "Windows HDR Calibration app", "Settings › Display › Color profile". Mention DisplayCAL or ArgyllCMS only to say Spectrum doesn't need them.

## Review

For every line: apply the delete test, the specificity test and the banned scan, then count words against the budget. Grep for `—`, "successfully", "please", "!" and the vocabulary list. Output a table of line, violation and rewritten line (or DELETE), then the corrected text. Rewrite; flagging alone is not enough.
