# Locally bundled typography

Schibsted Grotesk (400/500/600/800), Newsreader (400), and IBM Plex Mono (400/500)
are checked-in TrueType assets served by Next.js from the same origin. CSS uses
`font-display: swap`. No production font request goes to a third party, including
auth/token pages consuming this foundation.

Downloaded from Google Fonts on 2026-10-06 using the families' CSS font URLs.
License/provenance:

- [Schibsted Grotesk](https://github.com/google/fonts/tree/main/ofl/schibstedgrotesk): `schibsted-grotesk-OFL.txt`
- [Newsreader](https://github.com/google/fonts/tree/main/ofl/newsreader): `newsreader-OFL.txt`
- [IBM Plex Mono](https://github.com/google/fonts/tree/main/ofl/ibmplexmono): `ibm-plex-mono-OFL.txt`

Only the weights used by the foundation and its public typography are bundled.
Fonts are source assets; do not regenerate or fetch them during application runs.
