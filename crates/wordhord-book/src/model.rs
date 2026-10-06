//! A book's text as the vocabulary sees it (VOCABULARY.md): blocks, verse
//! lines and inlines, with Djot's syntax already resolved to meaning.

/// A block of text. Top-level blocks carry a stable ID.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: Option<String>,
    pub kind: BlockKind,
    pub lang: Option<String>,
    /// A canonical reference: a prefix for counted verse (`bw`), or a full
    /// reference (`vsp.1`, `nh.12.42`).
    pub reference: Option<String>,
    /// The 1-based line in the source file where the block starts.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u16,
        text: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    Verse(Verse),
    Stanza(Verse),
    Speech {
        speaker: Option<String>,
        blocks: Vec<Block>,
    },
    Epigraph(Vec<Block>),
    Letter(Vec<Block>),
    Quote(Vec<Block>),
    Center(Vec<Block>),
    Figure(Vec<Block>),
    SceneBreak,
    Table(Vec<Vec<Vec<Inline>>>),
    Note {
        label: String,
        blocks: Vec<Block>,
    },
    /// A note on a passage, keyed by its reference: `on="vsp.1"`.
    Commentary {
        on: String,
        blocks: Vec<Block>,
    },
}

/// Verse: lines, in groups separated by blank lines, and any stanzas
/// within it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Verse {
    pub start: Option<u32>,
    /// The book this aligns to, from `original=`.
    pub original: Option<String>,
    pub indent: u8,
    pub lines: Vec<VerseLine>,
    pub stanzas: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VerseLine {
    /// Indent steps, from leading spaces in the source (two spaces a step).
    pub indent: u8,
    /// Which group of lines this is in, counting from 0.
    pub group: usize,
    pub inlines: Vec<Inline>,
    /// The canonical reference, once numbered: `bw.4`, `bw.12-13`,
    /// `vsp.1.3`.
    pub reference: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    /// A span with meaning: small caps, a name, a gap, a line override, or
    /// just a language.
    Span {
        kind: SpanKind,
        lang: Option<String>,
        inlines: Vec<Inline>,
    },
    Link {
        target: String,
        inlines: Vec<Inline>,
    },
    Image {
        src: String,
        alt: Vec<Inline>,
    },
    NoteRef(String),
    /// The break between half-lines in verse.
    Caesura,
    LineBreak,
    /// A source line break inside a paragraph.
    SoftBreak,
    /// A page of the source edition starts here.
    PageBreak {
        n: String,
        edition: Option<String>,
    },
    /// A canonical section starts here, mid-block.
    SectionStart {
        reference: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpanKind {
    /// Only a language, or nothing else of note.
    Foreign,
    SmallCaps,
    Name {
        kind: NameKind,
        key: Option<String>,
        thing: Option<String>,
    },
    Gap,
    /// The editor's emendation of the manuscript.
    Corr,
    /// This verse line covers these lines of the original.
    LineOverride {
        from: u32,
        to: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Person,
    Place,
    Group,
    Named,
}

impl NameKind {
    pub fn class(self) -> &'static str {
        match self {
            NameKind::Person => "person",
            NameKind::Place => "place",
            NameKind::Group => "group",
            NameKind::Named => "named",
        }
    }
}

/// Plain text of inlines: a caesura as ` | `, breaks as spaces, markers as
/// nothing.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    push_plain(&mut s, inlines);
    s
}

fn push_plain(s: &mut String, inlines: &[Inline]) {
    for i in inlines {
        match i {
            Inline::Text(t) => s.push_str(t),
            Inline::Emphasis(c) | Inline::Strong(c) => push_plain(s, c),
            Inline::Span { inlines, .. } | Inline::Link { inlines, .. } => push_plain(s, inlines),
            Inline::Image { alt, .. } => push_plain(s, alt),
            Inline::Caesura => s.push_str(" | "),
            Inline::LineBreak | Inline::SoftBreak => s.push(' '),
            Inline::NoteRef(_) | Inline::PageBreak { .. } | Inline::SectionStart { .. } => {}
        }
    }
}

/// Every block in reading order, depth first, including blocks inside
/// containers and stanzas inside verse.
pub fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for b in blocks {
        out.push(b);
        match &b.kind {
            BlockKind::Speech { blocks, .. }
            | BlockKind::Epigraph(blocks)
            | BlockKind::Letter(blocks)
            | BlockKind::Quote(blocks)
            | BlockKind::Center(blocks)
            | BlockKind::Figure(blocks)
            | BlockKind::Note { blocks, .. }
            | BlockKind::Commentary { blocks, .. } => walk(blocks, out),
            BlockKind::Verse(v) | BlockKind::Stanza(v) => walk(&v.stanzas, out),
            _ => {}
        }
    }
}

/// Every inline list in a block itself (not in blocks inside it).
pub fn block_inlines(b: &Block) -> Vec<&[Inline]> {
    match &b.kind {
        BlockKind::Heading { text, .. } => vec![text.as_slice()],
        BlockKind::Paragraph(i) => vec![i.as_slice()],
        BlockKind::Verse(v) | BlockKind::Stanza(v) => {
            v.lines.iter().map(|l| l.inlines.as_slice()).collect()
        }
        BlockKind::Table(rows) => rows.iter().flatten().map(|c| c.as_slice()).collect(),
        _ => vec![],
    }
}

/// Every inline in a list, depth first.
pub fn walk_inlines<'a>(inlines: &'a [Inline], out: &mut Vec<&'a Inline>) {
    for i in inlines {
        out.push(i);
        match i {
            Inline::Emphasis(c) | Inline::Strong(c) => walk_inlines(c, out),
            Inline::Span { inlines, .. } | Inline::Link { inlines, .. } => {
                walk_inlines(inlines, out)
            }
            Inline::Image { alt, .. } => walk_inlines(alt, out),
            _ => {}
        }
    }
}
