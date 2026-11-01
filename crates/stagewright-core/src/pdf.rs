//! A small PDF writer for script pages: text in the three standard Courier
//! faces (no fonts to embed), underlines, and page numbers. Characters outside
//! Windows-1252 are printed as `?`.

use crate::layout::{Line, Style};

pub struct PdfPage {
    pub lines: Vec<Line>,
    /// Printed top right, e.g. "12."
    pub number: Option<String>,
}

pub struct PdfSettings {
    pub page_w: f32,
    pub page_h: f32,
    pub left: f32,
    pub top: f32,
    pub title: String,
    pub author: String,
}

/// Unicode -> Windows-1252 byte (PDF WinAnsiEncoding).
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        0x20..=0x7e => c as u8,
        0xa0..=0xff => c as u32 as u8,
        _ => match c {
            '€' => 0x80,
            '‚' => 0x82,
            'ƒ' => 0x83,
            '„' => 0x84,
            '…' => 0x85,
            '†' => 0x86,
            '‡' => 0x87,
            'ˆ' => 0x88,
            '‰' => 0x89,
            'Š' => 0x8a,
            '‹' => 0x8b,
            'Œ' => 0x8c,
            'Ž' => 0x8e,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '˜' => 0x98,
            '™' => 0x99,
            'š' => 0x9a,
            '›' => 0x9b,
            'œ' => 0x9c,
            'ž' => 0x9e,
            'Ÿ' => 0x9f,
            '\t' => b' ',
            _ => b'?',
        },
    }
}

/// A PDF string literal: (text) with \\ \( \) escaped, bytes in WinAnsi.
fn pdf_string(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let b = win_ansi(c);
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

const CHAR_W: f32 = 7.2; // Courier 12pt advance
const LINE_H: f32 = 12.0;

pub fn render(pages: &[PdfPage], s: &PdfSettings) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1 catalog, 2 pages, 3-5 fonts, 6 info; then per page: page object + content stream.
    let first_page_obj = 7;
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", first_page_obj + 2 * i))
        .collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).into_bytes());
    for face in ["Courier", "Courier-Bold", "Courier-Oblique"] {
        objects.push(
            format!("<< /Type /Font /Subtype /Type1 /BaseFont /{face} /Encoding /WinAnsiEncoding >>").into_bytes(),
        );
    }
    let mut info = b"<< /Producer (Stagewright) /Title ".to_vec();
    info.extend(pdf_string(&s.title));
    info.extend(b" /Author ");
    info.extend(pdf_string(&s.author));
    info.extend(b" >>");
    objects.push(info);

    for (i, page) in pages.iter().enumerate() {
        let content_obj = first_page_obj + 2 * i + 1;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {content_obj} 0 R >>",
                s.page_w, s.page_h
            )
            .into_bytes(),
        );
        let mut stream: Vec<u8> = Vec::new();
        let mut rules = String::new();
        let text_at = |stream: &mut Vec<u8>, font: &str, x: f32, y: f32, t: &str| {
            stream.extend(format!("BT /{font} 12 Tf {x:.2} {y:.2} Td ").as_bytes());
            stream.extend(pdf_string(t));
            stream.extend(b" Tj ET\n");
        };
        if let Some(n) = &page.number {
            let x = s.page_w - 72.0 - n.chars().count() as f32 * CHAR_W;
            text_at(&mut stream, "F1", x, s.page_h - 36.0 - 9.0, n);
        }
        for (row, line) in page.lines.iter().enumerate() {
            if line.text.is_empty() {
                continue;
            }
            let x = s.left + line.col as f32 * CHAR_W;
            let y = s.page_h - s.top - 9.0 - row as f32 * LINE_H;
            let font = match line.style {
                Style::Plain => "F1",
                Style::Bold => "F2",
                Style::Italic => "F3",
            };
            text_at(&mut stream, font, x, y, &line.text);
            if line.underline {
                let w = line.text.chars().count() as f32 * CHAR_W;
                rules.push_str(&format!("{x:.2} {:.2} m {:.2} {:.2} l S\n", y - 2.0, x + w, y - 2.0));
            }
        }
        if !rules.is_empty() {
            stream.extend(b"0.6 w\n");
            stream.extend(rules.as_bytes());
        }
        let mut obj = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        obj.extend(stream);
        obj.extend(b"\nendstream");
        objects.push(obj);
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(obj);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_quotes_and_escapes() {
        assert_eq!(pdf_string("a(b)\\"), b"(a\\(b\\)\\\\)".to_vec());
        assert_eq!(pdf_string("’…é"), vec![b'(', 0x92, 0x85, 0xe9, b')']);
        assert_eq!(pdf_string("日"), b"(?)".to_vec());
    }

    #[test]
    fn writes_a_well_formed_file() {
        let pages = vec![PdfPage {
            lines: vec![Line {
                text: "HELLO".into(),
                col: 0,
                style: Style::Plain,
                underline: true,
            }],
            number: None,
        }];
        let pdf = render(
            &pages,
            &PdfSettings {
                page_w: 612.0,
                page_h: 792.0,
                left: 108.0,
                top: 72.0,
                title: "T".into(),
                author: "A".into(),
            },
        );
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 1"));
        assert!(text.trim_end().ends_with("%%EOF"));
        // startxref points at the xref table, and each xref entry at its object (byte offsets).
        let xref_at: usize = text.lines().rev().nth(1).unwrap().parse().unwrap();
        assert!(pdf[xref_at..].starts_with(b"xref"));
        let table = String::from_utf8_lossy(&pdf[xref_at..]).to_string();
        for (n, line) in table.lines().skip(3).take(3).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{} 0 obj", n + 1).as_bytes()));
        }
    }
}
