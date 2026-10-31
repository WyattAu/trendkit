# trendkit-profile

Personal relevance profile for [trendkit](https://github.com/WyattAu/trendkit).

Builds language/topic histograms from a GitHub user's own repositories
(forks excluded) and ranks trending subjects by attributable overlap:
`2` per shared topic, `3` for a primary-language match, plus a
log-compressed velocity term; every point of the score is explainable.

Useful on day one: it needs no snapshot history, only one API listing.

See the [workspace README](https://github.com/WyattAu/trendkit).
