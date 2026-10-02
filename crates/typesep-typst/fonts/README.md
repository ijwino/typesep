# The embedded fonts

Every file here but this one is compiled into the binary (`EMBEDDED` in `src/fonts.rs`), so a
document typesets with no font installed. Each family's licence is beside it and is published
with every release.

## TeX Gyre

`texgyretermes-*.otf` (four faces and Termes Math), `texgyreheros-*.otf` and
`texgyrecursor-*.otf`, version 2.004 of each as their name tables say, are the body, sans,
monospaced and maths fonts of the presets. They are under the GUST Font License,
`GUST-FONT-LICENSE.txt`.

## Noto, for Arabic, Devanagari and Hebrew

The faces a document falls back to for scripts TeX Gyre has no letters for, and that are
small enough to carry (CJK, about 20 MB a face, is not: it stays a font the reader installs):

| File | Family, version |
| --- | --- |
| `NotoNaskhArabic-Regular.ttf`, `NotoNaskhArabic-Bold.ttf` | Noto Naskh Arabic 2.021 |
| `NotoSerifDevanagari-Regular.ttf`, `NotoSerifDevanagari-Bold.ttf` | Noto Serif Devanagari 2.006 |
| `NotoSerifHebrew-Regular.ttf`, `NotoSerifHebrew-Bold.ttf` | Noto Serif Hebrew 2.004 |

They are the hinted TrueType files of the Noto project's release repository,
[notofonts/notofonts.github.io](https://github.com/notofonts/notofonts.github.io), at commit
`e3ff34c3178cb4012124c9e6390b9a3535ff2c3f`, unchanged. They are under the SIL Open Font
License 1.1; each family's `OFL.txt`, with its copyright line, is here as

| File | From |
| --- | --- |
| `OFL-arabic.txt` | [notofonts/arabic](https://github.com/notofonts/arabic) at `834a11352b9ee26f827c745757dc380ef8df68bf` |
| `OFL-devanagari.txt` | [notofonts/devanagari](https://github.com/notofonts/devanagari) at `c4f7163256b8b0c3bf8a2d11fcf0314efc134989` |
| `OFL-hebrew.txt` | [notofonts/hebrew](https://github.com/notofonts/hebrew) at `a9e04ff21deca0b7814ab041886b90b296e7034f` |

Their SHA-256 sums, to check a copy against:

```
6f0a92031367b2f5a2078fe9d24f3433122b61a0bad57c423aad8f3c39aa2e6e  NotoNaskhArabic-Regular.ttf
04017b0ccdd1156dcde3bc386734c1b03165261762f16a025c8afc9088ae76e9  NotoNaskhArabic-Bold.ttf
e272d11d271c61a8d5d0f65764fd368ae53b9f9dabcce244a8941aa8f32ccc70  NotoSerifDevanagari-Regular.ttf
0544878a7b09ea1dc16d9de1f7a1f8cf82beb255d66a287712bb05a24336bcc0  NotoSerifDevanagari-Bold.ttf
dfd5a6aefe97a99f68fe43388342913d50bb9fbf6d3afc4d2c7725661bc4a2b1  NotoSerifHebrew-Regular.ttf
9dc92fcff313103397ec593cfda58e68f88ee71ff2d4a32aa9cff5a8b2725ea1  NotoSerifHebrew-Bold.ttf
a7a5a25eb188bf1cd96982030d53e23c33485c69b1044a562254226857ee13af  OFL-arabic.txt
a216f6f8d85c7228093e0ee5e258d9d377e6671f68acb4db1930b29583d0f331  OFL-devanagari.txt
e08036dbb38d9e7a033a721e0ca60befde2ed9d1a8075922dbd3429093dd793b  OFL-hebrew.txt
```

A family of `fonts.fallback` goes into a document's font lists only when the document has
characters the main and maths fonts lack and the family has, so a document in Latin script
names none of these and typesets as it did before they were here.
