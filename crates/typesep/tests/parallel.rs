use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Texts made up for these tests, written as the Wittgenstein Project's API answers for them
/// would be into a library of their own: a German one and its English translation, whose
/// remarks carry the same ids but two (one only the German has, one only the English), an
/// Arabic one written right to left and a Hindi one. The binary typesets the bilingual edition
/// of the first two in both layouts and as an EPUB, and the others on their own and as one
/// volume named in their scripts. With `TYPESEP_FIXTURE_OUT` naming a directory, the outputs
/// are left there for a look.
const OUT: &str = "TYPESEP_FIXTURE_OUT";

const SITE: &str = "https://www.wittgensteinproject.org/w/index.php/";

struct Words {
    subjects: &'static [&'static str],
    verbs: &'static [&'static str],
    objects: &'static [&'static str],
    /// Whether the verb comes last, as it does in Hindi.
    verb_last: bool,
    stop: &'static str,
}

const GERMAN: Words = Words {
    subjects: &[
        "Der Leuchtturm",
        "Das Logbuch",
        "Die Flut",
        "Der Wärter",
        "Das Licht",
        "Die Treppe",
        "Der Nebel",
    ],
    verbs: &["zeigt", "verbirgt", "zählt", "bewahrt", "beschreibt"],
    objects: &[
        "die Steine am Ufer",
        "jede Nacht von neuem",
        "was vor ihm war",
        "den Weg der Schiffe",
        "die Stunde der Ebbe",
    ],
    verb_last: false,
    stop: ".",
};

const ENGLISH: Words = Words {
    subjects: &[
        "The lighthouse",
        "The logbook",
        "The tide",
        "The keeper",
        "The light",
        "The stair",
        "The fog",
    ],
    verbs: &["shows", "hides", "counts", "keeps", "describes"],
    objects: &[
        "the stones on the shore",
        "each night anew",
        "what came before it",
        "the way of the ships",
        "the hour of the ebb",
    ],
    verb_last: false,
    stop: ".",
};

const ARABIC: Words = Words {
    subjects: &[
        "المنارة",
        "الدفتر",
        "المد",
        "الحارس",
        "الضوء",
        "الدرج",
        "الضباب",
    ],
    verbs: &["يظهر", "يخفي", "يعد", "يحفظ", "يصف"],
    objects: &[
        "حجارة الشاطئ",
        "كل ليلة من جديد",
        "ما كان قبله",
        "طريق السفن",
        "ساعة الجزر",
    ],
    verb_last: false,
    stop: ".",
};

const HINDI: Words = Words {
    subjects: &[
        "प्रकाशस्तंभ",
        "लेखा",
        "ज्वार",
        "रखवाला",
        "प्रकाश",
        "सीढ़ी",
        "कोहरा",
    ],
    verbs: &["दिखाता है", "छिपाता है", "गिनता है", "रखता है", "बताता है"],
    objects: &[
        "तट के पत्थर",
        "हर रात नए सिरे से",
        "जो पहले था",
        "जहाज़ों का रास्ता",
        "भाटे का समय",
    ],
    verb_last: true,
    stop: "।",
};

/// The `k`th sentence in `words`, and as many after it as `count` says.
fn sentences(words: &Words, k: usize, count: usize) -> String {
    (k..k + count)
        .map(|k| {
            let subject = words.subjects[k % words.subjects.len()];
            let verb = words.verbs[(k / 3) % words.verbs.len()];
            let object = words.objects[(k / 2) % words.objects.len()];
            if words.verb_last {
                format!("{subject} {object} {verb}{}", words.stop)
            } else {
                format!("{subject} {verb} {object}{}", words.stop)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The ids of the remarks: 1, 1.1, …, 6.7, as a decimal numbering runs.
fn remark_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for a in 1..=6 {
        ids.push(a.to_string());
        for b in 1..=7 {
            ids.push(format!("{a}.{b}"));
        }
    }
    ids
}

struct Text {
    title: &'static str,
    lang: &'static str,
    dir: &'static str,
    words: &'static Words,
    preface: &'static str,
    /// Remarks the text does not have.
    without: &'static [&'static str],
    /// Remarks only the text has, each after the remark it names.
    extra: &'static [(&'static str, &'static str)],
}

/// The text's page as the API renders it: the title page and colophon, a preface under a
/// heading, a remark for every id, some of several paragraphs, two with notes, one longer than
/// a page, and a rule with a paragraph after it inside a remark's unit.
fn page_html(text: &Text) -> String {
    let mut body = String::new();
    let heading = |words: &str| {
        format!(
            "<p style=\"display: block; font-size: 100%; font-family: -apple-system,'Arial',sans-serif; text-align: center;\"><i>{words}</i></p>\n"
        )
    };
    body.push_str(&heading(text.preface));
    body.push_str(&format!(
        "<p>{}</p>\n<p>{}</p>\n",
        sentences(text.words, 0, 6),
        sentences(text.words, 6, 5)
    ));
    let mut notes = String::new();
    let mut note = 0;
    let mut remark = |id: &str, k: usize, body: &mut String| {
        let mut first = sentences(text.words, k, 1 + k % 4);
        if k == 1 || k == 20 {
            note += 1;
            first.push_str(&format!(
                "<sup id=\"cite_ref-{note}\" class=\"reference\"><a href=\"#cite_note-{note}\"><span class=\"cite-bracket\">[</span>{note}<span class=\"cite-bracket\">]</span></a></sup>"
            ));
            notes.push_str(&format!(
                "<li id=\"cite_note-{note}\"><span class=\"mw-cite-backlink\"><a href=\"#cite_ref-{note}\">↑</a></span> <span class=\"reference-text\">{}</span></li>\n",
                sentences(text.words, k + 3, 2)
            ));
        }
        body.push_str(&format!(
            "<p><b><span class=\"tlp-aside-par\" id=\"{id}\">{id}</span></b> {first}</p>\n"
        ));
        let paragraphs = match k {
            12 => 14,
            _ if k.is_multiple_of(5) => 2,
            _ => 0,
        };
        for p in 0..paragraphs {
            body.push_str(&format!("<p>{}</p>\n", sentences(text.words, k + p, 4)));
        }
        if k == 30 {
            body.push_str(&format!("<hr />\n<p>{}</p>\n", sentences(text.words, k, 2)));
        }
    };
    for (k, id) in remark_ids().iter().enumerate() {
        if !text.without.contains(&id.as_str()) {
            remark(id, k, &mut body);
        }
        for (after, extra) in text.extra {
            if after == id {
                remark(extra, k + 1, &mut body);
            }
        }
    }
    format!(
        "<div class=\"mw-content-{dir} mw-parser-output\" lang=\"{lang}\" dir=\"{dir}\">\n\
         <div class=\"custom-print-only\"><p style=\"text-align:center;\">Nobody in Particular</p><p class=\"tl-title\">{title}</p></div>\n\
         <div class=\"colophon\"><p>Made up for typesep's tests; in the public domain.</p></div>\n\
         <p style=\"text-align:center;\">Nobody in Particular</p>\n<p class=\"tl-title\">{title}</p>\n\
         {body}<div class=\"mw-references-wrap\"><ol class=\"references\">\n{notes}</ol></div>\n</div>",
        dir = text.dir,
        lang = text.lang,
        title = text.title.replace('_', " "),
    )
}

const TEXTS: [Text; 4] = [
    Text {
        title: "Ein_Heft",
        lang: "de",
        dir: "ltr",
        words: &GERMAN,
        preface: "Vorwort",
        without: &[],
        extra: &[("4.3", "4.31")],
    },
    Text {
        title: "A_Notebook",
        lang: "en",
        dir: "ltr",
        words: &ENGLISH,
        preface: "Preface",
        without: &["4.31", "5.2"],
        extra: &[("5.2", "5.21")],
    },
    Text {
        title: "Daftar",
        lang: "ar",
        dir: "rtl",
        words: &ARABIC,
        preface: "مقدمة",
        without: &[],
        extra: &[],
    },
    Text {
        title: "Pustika",
        lang: "hi",
        dir: "ltr",
        words: &HINDI,
        preface: "प्रस्तावना",
        without: &[],
        extra: &[],
    },
];

/// The four texts in a library at `dir`, as `typesep fetch --source lwp` would have left them.
fn write_library(dir: &Path) {
    for (index, text) in TEXTS.iter().enumerate() {
        let unit = dir.join("lwp/pages").join(text.title);
        fs::create_dir_all(&unit).unwrap();
        let answer = serde_json::json!({
            "parse": {
                "title": text.title.replace('_', " "),
                "pageid": index + 1,
                "revid": 100 + index,
                "text": page_html(text),
                "displaytitle": format!("<span class=\"mw-page-title-main\">{}</span>", text.title.replace('_', " ")),
                "properties": {}
            }
        });
        fs::write(unit.join("parse.json"), answer.to_string()).unwrap();
        let meta = serde_json::json!({
            "url": format!("{SITE}{}", text.title),
            "title": text.title.replace('_', " "),
            "revision": (100 + index).to_string(),
            "files": { "parse.json": { "fetchedAt": "2026-10-02T09:00:00.000Z" } }
        });
        fs::write(unit.join("meta.json"), meta.to_string()).unwrap();
    }
}

/// Where the binary runs: a library of the texts, no system fonts, no network.
struct Harness {
    work: tempfile::TempDir,
}

impl Harness {
    fn new() -> Harness {
        let work = tempfile::tempdir().unwrap();
        let empty = work.path().join("no-fonts");
        fs::create_dir(&empty).unwrap();
        fs::write(
            work.path().join("fonts.conf"),
            format!(
                "<?xml version=\"1.0\"?>\n<fontconfig><dir>{}</dir></fontconfig>\n",
                empty.display()
            ),
        )
        .unwrap();
        write_library(&work.path().join("library"));
        Harness { work }
    }

    fn out(&self, name: &str) -> PathBuf {
        self.work.path().join("out").join(name)
    }

    fn typesep(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_typesep"))
            .current_dir(self.work.path())
            .env("FONTCONFIG_FILE", self.work.path().join("fonts.conf"))
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .env_remove("TYPESEP_FONTS")
            .env_remove("TYPESEP_LIBRARY")
            .args(args)
            .arg("--library")
            .arg(self.work.path().join("library"))
            .output()
            .unwrap()
    }

    /// Typesets with `args`, into `out/<name>/`, and says what it said on stderr.
    fn build(&self, name: &str, args: &[&str]) -> String {
        let out = self.out(name);
        let out_text = out.display().to_string();
        let mut all = vec!["--source", "lwp", "-o", &out_text, "--keep-typst", "-v"];
        all.extend(args);
        let run = self.typesep(&all);
        let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
        assert!(run.status.success(), "{args:?}:\n{stderr}");
        assert!(
            !stderr.lines().any(|line| line.starts_with("  GET ")),
            "nothing is fetched: {stderr}"
        );
        if let Some(dir) = std::env::var_os(OUT).filter(|dir| !dir.is_empty()) {
            let dir = PathBuf::from(dir).join(name);
            fs::create_dir_all(&dir).unwrap();
            for entry in fs::read_dir(&out).unwrap() {
                let entry = entry.unwrap();
                fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
            }
        }
        stderr
    }

    fn file(&self, name: &str, extension: &str) -> PathBuf {
        let found: Vec<PathBuf> = fs::read_dir(self.out(name))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == extension))
            .collect();
        assert_eq!(found.len(), 1, "{found:?}");
        found[0].clone()
    }

    fn typ(&self, name: &str) -> String {
        fs::read_to_string(self.file(name, "typ")).unwrap()
    }
}

fn pages(stderr: &str) -> usize {
    stderr
        .lines()
        .find_map(|line| line.trim().strip_suffix(" pages")?.parse().ok())
        .unwrap_or_else(|| panic!("a page count: {stderr}"))
}

#[test]
fn a_bilingual_edition_in_columns_sets_a_row_to_each_remark() {
    let harness = Harness::new();
    let stderr = harness.build(
        "columns",
        &[
            "--parallel",
            "Ein_Heft",
            "A_Notebook",
            "--format",
            "pdf,epub",
            "-s",
            "parallel.layout=columns",
        ],
    );
    assert!(
        stderr.contains(
            "3 remarks are in one text only and are set with nothing beside them: in Ein Heft 4.31, 5.2; in A Notebook 5.21"
        ),
        "{stderr}"
    );
    let typ = harness.typ("columns");
    assert!(
        harness
            .file("columns", "pdf")
            .ends_with("Ein_Heft+A_Notebook.pdf"),
        "named by the two works"
    );
    assert!(
        typ.contains("#heading(level: 1)[Ein Heft / A Notebook]"),
        "titled by both"
    );
    let rows = typ.matches("\n#fs-columns(").count();
    assert_eq!(
        rows,
        1 + 47 + 3,
        "the prefaces, the remarks both have, and those one has"
    );
    assert!(
        typ.contains("#fs-columns([#[\n#set text(lang: \"de\", dir: ltr)"),
        "{typ}"
    );
    assert!(
        typ.contains("#metadata(none)<left--1>") && typ.contains("#metadata(none)<right--1>"),
        "each side's ids apart"
    );
    assert!(pages(&stderr) > 3, "{stderr}");
}

#[test]
fn a_bilingual_edition_on_facing_pages_is_what_dump_typst_prints_and_its_epub_has_rows() {
    let harness = Harness::new();
    let stderr = harness.build(
        "facing",
        &[
            "--parallel",
            "Ein_Heft",
            "A_Notebook",
            "--format",
            "pdf,epub",
        ],
    );
    assert!(
        stderr.contains("remark 2.4 is longer than a page, so it starts a spread"),
        "{stderr}"
    );
    assert_eq!(
        pages(&stderr) % 2,
        0,
        "a colophon after the last recto: {stderr}"
    );
    let typ = harness.typ("facing");
    assert!(typ.contains("#pagebreak(to: \"odd\")"), "spreads");
    assert!(!typ.contains("\n#fs-columns("), "no rows of columns");
    let dump = harness.typesep(&[
        "dump",
        "typst",
        "--source",
        "lwp",
        "--parallel",
        "Ein_Heft",
        "A_Notebook",
        "--quiet",
    ]);
    assert!(dump.status.success());
    assert_eq!(
        String::from_utf8_lossy(&dump.stdout),
        typ,
        "dump typst prints the source the PDF was made of"
    );

    let epub = fs::File::open(harness.file("facing", "epub")).unwrap();
    let mut archive = zip::ZipArchive::new(epub).unwrap();
    let mut page = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("OEBPS/entry-1.xhtml").unwrap(),
        &mut page,
    )
    .unwrap();
    assert!(
        page.contains("<div class=\"columns\"><div class=\"column\"><div lang=\"de\" xml:lang=\"de\" dir=\"ltr\">"),
        "the EPUB sets the rows whatever the layout: {page}"
    );
    assert_eq!(page.matches("<div class=\"columns\">").count(), 51);
}

#[test]
fn a_text_written_right_to_left_and_one_in_hindi_typeset_on_their_own() {
    let harness = Harness::new();
    let stderr = harness.build("arabic", &["Daftar", "--format", "pdf,epub"]);
    assert!(
        stderr.contains("Typst has no hyphenation patterns for ar, the document's language"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("no font"),
        "Noto Naskh Arabic has it all: {stderr}"
    );
    let typ = harness.typ("arabic");
    assert!(typ.contains("#set text(lang: \"ar\", dir: rtl, hyphenate: false)"));
    assert!(typ.contains("#let fs-main = (\"TeX Gyre Termes\", \"Noto Naskh Arabic\")"));

    let stderr = harness.build("hindi", &["Pustika", "--format", "pdf,epub"]);
    assert!(!stderr.contains("no font"), "{stderr}");
    assert!(
        !stderr.contains("hyphenation"),
        "Typst hyphenates Hindi: {stderr}"
    );
    let typ = harness.typ("hindi");
    assert!(typ.contains("#set text(lang: \"hi\", hyphenate: true)"));
    assert!(typ.contains("#let fs-main = (\"TeX Gyre Termes\", \"Noto Serif Devanagari\")"));
}

#[test]
fn the_source_the_pdf_and_the_epub_take_one_name_whatever_its_script() {
    let harness = Harness::new();
    let name = "Daftar (دفتر) और पुस्तिका";
    let stderr = harness.build(
        "named",
        &["--merge", name, "Daftar", "Pustika", "--format", "pdf,epub"],
    );
    for extension in ["typ", "pdf", "epub"] {
        let file = harness.file("named", extension);
        assert_eq!(
            file.file_stem().unwrap().to_string_lossy(),
            name,
            "{}",
            file.display()
        );
    }
    // The path is shown relative to the working directory where it can be, which depends on
    // the platform's separators and its temporary directory; its file name does not.
    let kept = stderr
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("source in "))
        .unwrap_or_else(|| panic!("the source kept is named: {stderr}"));
    assert_eq!(
        Path::new(kept).file_name().unwrap().to_string_lossy(),
        format!("{name}.typ"),
        "{stderr}"
    );
}

#[test]
fn a_bilingual_edition_of_a_text_and_one_written_right_to_left() {
    let harness = Harness::new();
    for layout in ["facing", "columns"] {
        let set = format!("parallel.layout={layout}");
        harness.build(
            &format!("arabic-{layout}"),
            &[
                "--parallel",
                "Ein_Heft",
                "Daftar",
                "--format",
                "pdf,epub",
                "-s",
                &set,
            ],
        );
        let typ = harness.typ(&format!("arabic-{layout}"));
        assert!(typ.contains("#set text(lang: \"ar\", hyphenate: false, dir: rtl)"));
        assert!(
            typ.contains("#set text(lang: \"de\", dir: ltr)"),
            "each side in its own direction"
        );
    }
}

#[test]
fn a_bilingual_edition_takes_two_texts_with_remarks() {
    let harness = Harness::new();
    let run = harness.typesep(&["--source", "lwp", "--parallel", "Ein_Heft"]);
    assert_eq!(run.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&run.stderr)
            .contains("--parallel takes two works, the one for the left first, and 1 was given")
    );
}
