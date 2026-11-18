mod common;

#[test]
fn make_pdf_should_produce_pdf_with_requested_page_count() {
    let bytes = common::make_pdf(&["Page 1", "Page 2", "Page 3"]);
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("fixture parses");
    assert_eq!(pdf.pages().len(), 3);
}
