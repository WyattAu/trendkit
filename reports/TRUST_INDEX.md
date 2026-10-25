# trendkit trust index — 2026-10-25

Generated from 1737 tracked subjects and 12641 observations over a 8-day window by [trendkit](https://crates.io/crates/trendkit). Every number below is recomputed from the raw snapshot store; the code that computes it is public.

## Flagged subjects: 97 above the Elevated fork-ratio line

Trust blends four signals: fork ratio, membership in an org burst, velocity-outlier z, and subject age. Sorted worst first; `trust` runs 0-100.

| repo | stars | forks | ratio | trust |
|---|---:|---:|---:|---:|
| x4gpanell/JinX-X4G | 371 | 837 | 226% | 10 |
| storytold/craft-libs | 325 | 620 | 191% | 0 |
| WoYun520/dsh-purge | 434 | 722 | 166% | 33 |
| storytold/craft-fonts | 360 | 590 | 164% | 10 |
| Hinln/ARTEX | 907 | 1,392 | 153% | 39 |
| mmparko/dsh-purge | 2,242 | 2,796 | 125% | 40 |
| heqiangdiao-wq/AI-Cross-Border-Ecommerce-Agent-Toolkit | 639 | 681 | 107% | 57 |
| CrocSwap/integer-mult-bounds | 289 | 299 | 103% | 59 |
| jddevtime/open-lakehouse-compute-fabric | 276 | 275 | 100% | 60 |
| fort-tool/.github | 270 | 269 | 100% | 60 |
| cod-tool/.github | 270 | 268 | 99% | 60 |
| cs2-tools-new/.github | 268 | 264 | 99% | 60 |
| val-tools/.github | 244 | 238 | 98% | 61 |
| cs-tool-s/.github | 244 | 216 | 89% | 64 |
| JeffKeeping/dsh-purge-plus | 435 | 376 | 86% | 65 |
| crhywun/gpt-subscribe-studio-1 | 246 | 206 | 84% | 66 |
| apex-pro12/.github | 262 | 214 | 82% | 67 |
| rbl-tools/.github | 242 | 197 | 81% | 67 |
| david-silvalab1993i0/Roblox-Aimbot-Smooth-Tracking-Auto-Aim-Script-2026 | 304 | 217 | 71% | 71 |
| RuoJi6/ARTEX | 490 | 324 | 66% | 74 |

## Name-cluster farms

Repo names ending in `/.github` with a best-known fork ratio over 20 percent, counted per day. These are template-deployed farms: one repository cloned across freshly created orgs, stars bought in batches. Sources observe members sporadically, so each day counts every member whose last known state had cleared the line.

| day | cluster size |
|---|---:|
| 2026-10-20 | 5 |
| 2026-10-21 | 5 |
| 2026-10-22 | 8 |
| 2026-10-23 | 8 |

The cluster grew from 5 to 8 orgs across this window.

## Method

- Fork ratio = forks per 100 stars at the latest snapshot; Elevated starts at 20 percent, Extreme near 100.
- Trust blends fork ratio, org-burst membership (3+ same-shape repos created within 5 days), velocity-outlier z, and subject age. Extreme fork verdicts cap the trust band at Watch.
- A high fork ratio alone is not fraud: this page only claims that the engagement pattern is farm-shaped. The named clusters' ratios (81-100 percent) are far beyond organic baselines (this store's median is ~7).
- Known limits: bought stars that stay under the ratio line are invisible here; deleted repos leave gaps.
