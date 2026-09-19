# The vendored mitex scope

`specs/` is the Typst half of the [mitex](https://github.com/mitex-rs/mitex) package 0.2.4
(Apache-2.0, `LICENSE` beside it): the tables that give every LaTeX command a Typst meaning. The
`mitex` Rust crate converts a formula into Typst maths that names these; `sep-typst` serves the
files to the compiler under `/mitex/specs/` and evaluates each formula in their scope.

The package was written against Typst 0.11 and we link 0.15.1, so the copies carry three edits.
`tests/scope.rs` compiles every alias in the scope and fails when one no longer resolves — or
resolves with a deprecation, which is the same thing one release later — and that is what found
them and what will find the next ones.

1. **`latex/standard.typ`, first line.** The upstream file imports `@preview/xarrow` from Typst
   Universe for the stretchable arrows (`\xrightarrow` and friends). A package would have to be
   fetched, and the binary is one file and nothing beside it, so the import is replaced by a two-line
   definition over `math.stretch` and `math.attach`.

2. **`latex/standard.typ`, `KaTeX`.** The logo was spelled `$kai A T E X$`; `kai` is no longer a
   symbol Typst resolves in maths, and the alias is for `\KaTeX`, which no entry uses.

3. **`latex/standard.typ`, the `\bra` and `\ket` family.** The handlers spelled their brackets
   `angle.l` and `angle.r`, which Typst has renamed to `chevron.l` and `chevron.r`. A handler body
   is Typst that runs when a formula calls it, and `math::rename_symbols` never sees it: the renames
   below are applied to what the `mitex` crate emits, not to these files. That is why this one had to
   be an edit, and why `tests/scope.rs` also checks the files themselves for a name in the table.

Beside the edits, Typst has renamed **37 symbols** since 0.11. The `mitex` crate converts against a
table compiled into it, so those are renamed on its output by `math::rename_symbols` and the files
here are left alone. The list, from `codex`'s `sym.txt`:

| was | is |
| --- | --- |
| `sect`, `sect.big`, `sect.double`, `sect.sq` | `inter`, `inter.big`, `inter.double`, `inter.sq` |
| `angle.l`, `angle.r` | `chevron.l`, `chevron.r` |
| `bracket.l.double`, `bracket.r.double` | `bracket.l.stroked`, `bracket.r.stroked` |
| `arrow.l.dash`, `arrow.r.dash` | `arrow.l.open`, `arrow.r.open` |
| `ast.circle`, `dash.circle` | `ast.o`, `dash.o` |
| `circle.nested` | `compose.o` |
| `dot.circle`, `dot.circle.big` | `dot.o`, `dot.o.big` |
| `plus.circle`, `plus.circle.big`, `minus.circle` | `plus.o`, `plus.o.big`, `minus.o` |
| `times.circle`, `times.circle.big` | `times.o`, `times.o.big` |
| `diff` | `partial` |
| `ohm.inv` | `Omega.inv` |
| `planck.reduce` | `planck` |
| `tack.r.double`, `tack.r.double.not` | `tack.rr`, `tack.rr.not` |
| `tack.l.double`, `tack.t.double`, `tack.b.double` | `tack.ll`, `tack.tt`, `tack.bb` |
| `gt.tri`, `gt.tri.eq`, `gt.tri.eq.not`, `gt.tri.not` | `gt.closed`, `gt.closed.eq`, `gt.closed.eq.not`, `gt.closed.not` |
| `lt.tri`, `lt.tri.eq`, `lt.tri.eq.not`, `lt.tri.not` | `lt.closed`, `lt.closed.eq`, `lt.closed.eq.not`, `lt.closed.not` |
| `join` | `bowtie.big` |

The last ten are deprecations rather than removals: Typst still resolves them, with a warning, and
the warning is what `tests/scope.rs` catches.

To take a newer mitex: copy `packages/mitex/specs/` from the release whose version matches the
`mitex` crate in `Cargo.toml`, apply edits 1 and 3, run `cargo test -p sep-typst --test scope`,
and write down here whatever it turns up.

What the scope cannot be made to do is read LaTeX mitex does not know at all. `math::for_mitex`
rewrites that before the crate sees it: `\kern` and its family dropped, `ex` and `mu` lengths turned
into `em`, `{a} \over {b}` written out as `\frac{a}{b}`, and an `@{…}` column separator dropped from
an array preamble when it holds only space.
