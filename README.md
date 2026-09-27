# typesep

Give it the name of a [Stanford Encyclopedia of Philosophy (SEP)](https://plato.stanford.edu/)
entry, and it fetches the entry and typesets it as a PDF or an EPUB, to read on paper or on
an e-reader.

```
$ typesep computer-science
out/computer-science.pdf
```

## Supporting the SEP

> This project is not affiliated with the Stanford Encyclopedia of Philosophy.

The SEP is free to read because other people pay for it, so if you get some use out of
this, they are worth supporting:

- [Friends of the SEP Society](https://plato.stanford.edu/support/friends.html) is the
  individual membership, and members can download the SEP's own PDFs of entries.
- [SEPIA](https://plato.stanford.edu/support/sepia.html) is the program for libraries and
  academic institutions.
- [Donations](https://plato.stanford.edu/support/donate.html) go to the SEP's endowment.

## Install

Download the file for your computer from the
[latest release](https://github.com/ijwino/typesep/releases/latest) and run it.

If you would rather build it yourself, you need a recent stable Rust and nothing else:

```sh
cargo build --release
make test
```

## Usage

```sh
typesep turing-machine                                   # writes out/turing-machine.pdf
typesep --format epub --preset ereader turing-machine    # an EPUB sized for e-readers
typesep --merge "Logic" logic-classical logic-modal      # bundles two entries into one volume
typesep kant --paper a4 --font "EB Garamond" --leading 1.3
typesep kant -s page.margins.top=1in -s toc.pageNumbers=true
```

You can name an entry by its slug, the last part of its address, or by the whole address if
that is easier to copy.

`typesep --help` lists every command and flag, and
[docs/style-reference.md](docs/style-reference.md) every style option you can set.

## License

typesep is MIT ([LICENSE](LICENSE)). The TeX Gyre fonts inside it are under the
[GUST Font License](crates/sep-typst/fonts/GUST-FONT-LICENSE.txt), and the licenses of the
crates it is built from come with each release as `THIRD-PARTY-LICENSES.txt`.

### The entries

The entries themselves are copyright the Stanford Encyclopedia of Philosophy and their
authors, under the [SEP's Terms of Use](https://plato.stanford.edu/info.html#terms). You
may read, download, print and copy an entry for yourself. Redistributing copies to other
people is restricted, so have a look at the terms before you do. You can find this on the
last page of each PDF and in each EPUB's `dc:rights`.
