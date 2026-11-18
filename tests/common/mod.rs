//! Helpers shared by the integration tests.

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};

/// Builds a PDF in memory: one US Letter page per string in `pages`.
///
/// Each page draws its text in Helvetica 24 pt starting at (72, 720); a `\n`
/// in the text starts a new line 30 pt lower. An empty string gives a blank page.
pub fn make_pdf(pages: &[&str]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });

    let mut kids: Vec<Object> = Vec::new();
    for text in pages {
        let mut operations = Vec::new();
        if !text.is_empty() {
            operations.push(Operation::new("BT", vec![]));
            operations.push(Operation::new("Tf", vec!["F1".into(), 24.into()]));
            operations.push(Operation::new("TL", vec![30.into()]));
            operations.push(Operation::new("Td", vec![72.into(), 720.into()]));
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    // `T*` moves to the next line, using the leading set by `TL`.
                    operations.push(Operation::new("T*", vec![]));
                }
                operations.push(Operation::new("Tj", vec![Object::string_literal(line)]));
            }
            operations.push(Operation::new("ET", vec![]));
        }
        let content = Content { operations };
        let encoded = content.encode().expect("content stream encodes");
        let content_id = doc.add_object(Stream::new(dictionary! {}, encoded));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font_id },
            },
        });
        kids.push(page_id.into());
    }

    let count = i64::try_from(kids.len()).expect("page count fits in i64");
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("PDF serializes");
    bytes
}
